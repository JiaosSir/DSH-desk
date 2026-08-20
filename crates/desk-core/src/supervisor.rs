//! sidecar 监督状态机。
//!
//! 语义（与规格决策 3 对齐）：
//! - `start(port, …)` 拉起子进程（attempt 计数 +1，重置 `stopped` 标记）；
//! - `wait()` 消费子进程输出与退出：
//!   - 抓到就绪 URL 行 → `Ready{url}`（并标记「已就绪」，此后崩溃视为运行中崩溃）；
//!   - **就绪前**退出/超时 → `Exited{attempt}`，由壳换端口重试（端口冲突场景）；
//!   - **就绪后**崩溃 → 按退避序列（1s/2s/4s）以同端口自动重启，直至耗尽上限；
//!   - 尝试耗尽 → `Failed{reason}`（壳渲染错误页）；
//! - `stop()` 是有意停止：kill 子进程，`wait()` 返回 `Stopped`，不再自动重启。

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::ready::extract_ready_url;

/// 把一个输出流挂进行读取任务：逐行喂输出缓冲与日志管道。
fn spawn_reader(
    stream: impl AsyncRead + Unpin + Send + 'static,
    tx: mpsc::UnboundedSender<String>,
    output: Arc<Mutex<String>>,
    sink: Option<mpsc::UnboundedSender<String>>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            output.lock().expect("输出缓冲锁").push_str(&line);
            output.lock().expect("输出缓冲锁").push('\n');
            if let Some(sink) = &sink {
                let _ = sink.send(line.clone());
            }
            let _ = tx.send(line);
        }
    });
}

/// 监督器向壳上报的事件。
#[derive(Debug, Clone, PartialEq)]
pub enum SupervisorEvent {
    /// sidecar 打印了就绪 URL 行。
    Ready { url: String },
    /// 子进程意外退出（`code = None` 表示被信号/超时终止）；`attempt` 是失败
    /// 那次尝试的序号。就绪前的退出由壳换端口重试；就绪后的退出监督器已
    /// 自动重启，此事件仅作镜像。
    Exited { code: Option<i32>, attempt: u32 },
    /// 尝试耗尽，进入终态（壳渲染错误页）。
    Failed { reason: String },
    /// `stop()` 请求的停止已完成。
    Stopped,
}

/// 监督参数。
#[derive(Debug, Clone)]
pub struct SupervisorOptions {
    /// 就绪等待超时（与官方 e2e 一致：90s）。
    pub ready_timeout: Duration,
    /// 最大尝试次数（含首次）。
    pub max_attempts: u32,
    /// 崩溃重启退避序列（[1s, 2s, 4s]）。
    pub backoff: Vec<Duration>,
}

impl Default for SupervisorOptions {
    fn default() -> Self {
        Self {
            ready_timeout: Duration::from_secs(90),
            max_attempts: 3,
            backoff: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ],
        }
    }
}

/// 一次运行中的子进程及其行事件流。
struct Running {
    child: Child,
    line_rx: mpsc::UnboundedReceiver<String>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // 取消安全性：wait() 被外部的 select 取消时，Running 就地析构，
        // 必须 kill 子进程——否则留下孤儿 sidecar 与旧端口占用。
        let _ = self.child.start_kill();
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        // 监督器销毁时不留下孤儿 sidecar：尽力 kill 当前子进程。
        // 这也让 tokio 在 Windows 上经 blocking pool 阻塞读管道的任务
        // 随子进程退出而解除阻塞（否则 runtime 关闭会卡在排空上）。
        if let Some(running) = &mut self.running {
            let _ = running.child.start_kill();
        }
    }
}

/// `wait()` 内部 select 的产出标记。
enum WaitStep {
    /// 行流事件（None = 流已关闭，此后不再有行）。
    Line(Option<String>),
    /// 子进程退出。
    Exit(Option<std::process::ExitStatus>),
    /// 就绪等待超时。
    Timeout,
}

/// sidecar 监督器：spawn → 等就绪 → 运行；崩溃退避重启，耗尽后终态。
pub struct Supervisor {
    options: SupervisorOptions,
    running: Option<Running>,
    program: String,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    port: u16,
    attempts: u32,
    stopped: bool,
    /// 就绪后首次崩溃：同端口自动重启（WebView 的 URL 保持不变）。
    ever_ready: bool,
    /// 累计输出（错误页尾部展示）。
    output: Arc<Mutex<String>>,
    /// 可选日志管道：每条原始行都会转发（壳接 logs::append）。
    log_sink: Option<mpsc::UnboundedSender<String>>,
}

impl Supervisor {
    /// 以给定参数创建监督器。
    pub fn new(options: SupervisorOptions) -> Self {
        Self {
            options,
            running: None,
            program: String::new(),
            args: Vec::new(),
            envs: Vec::new(),
            port: 0,
            attempts: 0,
            stopped: false,
            ever_ready: false,
            output: Arc::new(Mutex::new(String::new())),
            log_sink: None,
        }
    }

    /// 挂接日志管道：之后每条 sidecar 原始输出行都会转发给 `sink`。
    pub fn set_log_sink(&mut self, sink: mpsc::UnboundedSender<String>) {
        self.log_sink = Some(sink);
    }

    /// 累计输出（错误页显示尾部）。
    pub fn output(&self) -> String {
        self.output.lock().expect("输出缓冲锁").clone()
    }

    /// 当前尝试序号（0 表示尚未启动）。
    pub fn attempt_count(&self) -> u32 {
        self.attempts
    }

    /// 当前端口（None 表示尚未启动）。
    pub fn current_port(&self) -> Option<u16> {
        if self.attempts == 0 {
            None
        } else {
            Some(self.port)
        }
    }

    /// 子进程是否仍在（且非有意停止）。
    pub fn is_running(&self) -> bool {
        self.running.is_some() && !self.stopped
    }

    /// 以指定端口拉起 sidecar。attempt 计数 +1；已有子进程时先 kill 回收。
    pub async fn start(
        &mut self,
        port: u16,
        program: &str,
        args: &[&str],
        envs: &[(String, String)],
    ) -> Result<(), String> {
        if let Some(mut old) = self.running.take() {
            let _ = old.child.start_kill();
            let _ = old.child.wait().await;
        }
        self.port = port;
        self.program = program.to_owned();
        self.args = args.iter().map(|s| s.to_string()).collect();
        self.envs = envs.to_vec();
        self.stopped = false;
        self.attempts += 1;
        let running = self.spawn_child().await?;
        self.running = Some(running);
        Ok(())
    }

    /// 有意停止：kill 子进程并标记；`wait()` 会返回 `Stopped`，不自动重启。
    pub fn stop(&mut self) {
        self.stopped = true;
        if let Some(running) = &mut self.running {
            let _ = running.child.start_kill();
        }
    }

    /// 消费一个事件：就绪 / 退出（含自动重启）/ 失败 / 停止。
    pub async fn wait(&mut self) -> SupervisorEvent {
        let Some(mut running) = self.running.take() else {
            // 无子进程（尚未 start 或已停止回收）→ 视为已停止。
            return SupervisorEvent::Stopped;
        };
        // 行流关闭后（子进程自行关掉了输出）不再轮询行分支，
        // 避免 recv() 永远立即返回 None 造成忙等。
        let mut lines_closed = false;
        loop {
            // select 放在嵌套块里：各 future 对 running 的借用随块结束而
            // 释放，之后才能安全地把 running 移回 self。
            let step = {
                let line_next = async { running.line_rx.recv().await };
                tokio::pin!(line_next);
                let timeout = tokio::time::sleep(self.options.ready_timeout);
                tokio::pin!(timeout);
                tokio::select! {
                    line = &mut line_next, if !lines_closed => WaitStep::Line(line),
                    status = running.child.wait() => WaitStep::Exit(status.ok()),
                    // 就绪超时只约束"等待就绪"阶段：就绪后宿主静默运行是
                    // 常态（无输出不代表崩溃），不能再计时。
                    _ = &mut timeout, if !self.ever_ready => WaitStep::Timeout,
                }
            };

            match step {
                WaitStep::Line(None) => {
                    lines_closed = true;
                    // running 保持在本循环，继续等退出/超时。
                }
                WaitStep::Line(Some(line)) => {
                    if let Some(url) = extract_ready_url(&line) {
                        self.ever_ready = true;
                        self.running = Some(running);
                        return SupervisorEvent::Ready { url };
                    }
                    // 非就绪行：继续等下一行。
                }
                WaitStep::Exit(status) => {
                    let code = status.and_then(|s| s.code());
                    if self.stopped {
                        return SupervisorEvent::Stopped;
                    }
                    if self.attempts >= self.options.max_attempts {
                        let reason =
                            format!("宿主连续启动失败 {} 次（退出码 {:?}）", self.attempts, code);
                        return SupervisorEvent::Failed { reason };
                    }
                    let failed_attempt = self.attempts;
                    if self.ever_ready {
                        // 运行中崩溃：按退避序列以同端口自动重启（URL 不变）。
                        let backoff = self
                            .options
                            .backoff
                            .get((self.attempts - 1) as usize)
                            .copied()
                            .unwrap_or(Duration::from_secs(4));
                        tokio::time::sleep(backoff).await;
                        self.attempts += 1;
                        match self.spawn_child().await {
                            Ok(new_running) => self.running = Some(new_running),
                            Err(e) => return SupervisorEvent::Failed { reason: e },
                        }
                        return SupervisorEvent::Exited {
                            code,
                            attempt: failed_attempt,
                        };
                    }
                    // 就绪前退出（含端口冲突）：壳换端口重试。
                    return SupervisorEvent::Exited {
                        code,
                        attempt: failed_attempt,
                    };
                }
                WaitStep::Timeout => {
                    // 就绪超时：杀掉当前子进程，按一次失败尝试处理。
                    let _ = running.child.start_kill();
                    let _ = running.child.wait().await;
                    if self.attempts >= self.options.max_attempts {
                        return SupervisorEvent::Failed {
                            reason: "就绪等待超时".to_owned(),
                        };
                    }
                    let failed_attempt = self.attempts;
                    return SupervisorEvent::Exited {
                        code: None,
                        attempt: failed_attempt,
                    };
                }
            }
        }
    }

    /// 按当前配置拉起子进程：stdout/stderr 逐行喂输出缓冲与日志管道。
    async fn spawn_child(&mut self) -> Result<Running, String> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .envs(self.envs.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("sidecar 启动失败: {e}"))?;
        let stdout = child.stdout.take().expect("已配置 piped 的 stdout");
        let stderr = child.stderr.take().expect("已配置 piped 的 stderr");

        let (tx, rx) = mpsc::unbounded_channel();
        let output = Arc::clone(&self.output);
        let sink = self.log_sink.clone();
        // stdout 与 stderr 各一个行读取任务，汇入同一事件流。
        spawn_reader(stdout, tx.clone(), Arc::clone(&output), sink.clone());
        spawn_reader(stderr, tx, Arc::clone(&output), sink);
        Ok(Running { child, line_rx: rx })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use std::time::Duration;

    /// 测试环境是否可用 node（假 sidecar 的载体）；无 node 时静默跳过。
    fn node_available() -> bool {
        StdCommand::new("node")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// 长驻假 sidecar：打印就绪行后挂住。
    fn ready_node_script(port: u16) -> String {
        format!("console.log('dsh web: http://127.0.0.1:{port}'); setInterval(()=>{{}},1000)")
    }

    fn fast_options() -> SupervisorOptions {
        SupervisorOptions {
            ready_timeout: Duration::from_secs(5),
            max_attempts: 3,
            backoff: vec![
                Duration::from_millis(50),
                Duration::from_millis(50),
                Duration::from_millis(50),
            ],
        }
    }

    /// 启动一个假 sidecar 并消费一个事件。
    async fn start_and_wait(script: &str, port: u16) -> (Supervisor, SupervisorEvent) {
        let mut sup = Supervisor::new(fast_options());
        sup.start(port, "node", &["-e", script], &[])
            .await
            .expect("start 成功");
        let evt = sup.wait().await;
        (sup, evt)
    }

    #[tokio::test]
    async fn 打印就绪行后返回_ready() {
        if !node_available() {
            return;
        }
        let port = crate::ports::pick_free_port().expect("空闲端口");
        let (sup, evt) = start_and_wait(&ready_node_script(port), port).await;
        assert_eq!(
            evt,
            SupervisorEvent::Ready {
                url: format!("http://127.0.0.1:{port}")
            }
        );
        assert!(sup.is_running(), "就绪后子进程应仍在运行");
        let mut sup = sup;
        sup.stop();
        assert_eq!(sup.wait().await, SupervisorEvent::Stopped);
        assert!(!sup.is_running(), "停止后不再运行");
    }

    #[tokio::test]
    async fn 立即退出返回_exited() {
        if !node_available() {
            return;
        }
        let port = crate::ports::pick_free_port().expect("空闲端口");
        let (sup, evt) = start_and_wait("process.exit(3)", port).await;
        assert_eq!(
            evt,
            SupervisorEvent::Exited {
                code: Some(3),
                attempt: 1
            }
        );
        assert_eq!(sup.attempt_count(), 1);
        assert!(!sup.is_running());
    }

    #[tokio::test]
    async fn 端口冲突后换端口重启可_ready() {
        if !node_available() {
            return;
        }
        // 占住一个端口，模拟 sidecar 绑定冲突。
        let taken = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("占住端口");
        let taken_port = taken.local_addr().expect("取端口").port();

        let mut sup = Supervisor::new(fast_options());
        sup.start(
            taken_port,
            "node",
            &["-e", "console.error('EADDRINUSE'); process.exit(1)"],
            &[],
        )
        .await
        .expect("start 成功");
        assert_eq!(
            sup.wait().await,
            SupervisorEvent::Exited {
                code: Some(1),
                attempt: 1
            }
        );

        // 壳侧逻辑：换一个空闲端口重试（本测试替代壳的行为）。
        let new_port = crate::ports::pick_free_port().expect("新空闲端口");
        sup.start(new_port, "node", &["-e", &ready_node_script(new_port)], &[])
            .await
            .expect("重启成功");
        let evt = sup.wait().await;
        assert!(
            matches!(evt, SupervisorEvent::Ready { .. }),
            "换端口后应就绪: {evt:?}"
        );
        assert_eq!(sup.attempt_count(), 2, "attempt 应为 2");
        // 测试结束时监督器 Drop 会 kill 长驻子进程，保证 runtime 正常收尾。
        let _ = taken; // 保持占用至测试结束
    }

    #[tokio::test]
    async fn 连续失败耗尽后返回_failed() {
        if !node_available() {
            return;
        }
        let mut sup = Supervisor::new(fast_options());
        let mut evt = SupervisorEvent::Stopped;
        for round in 1..=3 {
            let port = crate::ports::pick_free_port().expect("空闲端口");
            sup.start(port, "node", &["-e", "process.exit(7)"], &[])
                .await
                .expect("start 成功");
            evt = sup.wait().await;
            if round < 3 {
                assert_eq!(
                    evt,
                    SupervisorEvent::Exited {
                        code: Some(7),
                        attempt: round
                    },
                    "第 {round} 轮应为 Exited"
                );
            }
        }
        assert!(
            matches!(evt, SupervisorEvent::Failed { .. }),
            "第三轮应为 Failed: {evt:?}"
        );
        assert!(!sup.is_running());
    }

    #[tokio::test]
    async fn 就绪后崩溃自动同端口重启() {
        if !node_available() {
            return;
        }
        let port = crate::ports::pick_free_port().expect("空闲端口");
        // 打印就绪行后立刻退出（模拟运行中崩溃）。
        let script = format!("console.log('dsh web: http://127.0.0.1:{port}'); process.exit(0)");
        let mut sup = Supervisor::new(fast_options());
        sup.start(port, "node", &["-e", &script], &[])
            .await
            .expect("start 成功");
        assert!(matches!(sup.wait().await, SupervisorEvent::Ready { .. }));

        // 就绪后的退出：监督器自动以同端口重启，事件镜像 Exited。
        let evt = sup.wait().await;
        assert_eq!(
            evt,
            SupervisorEvent::Exited {
                code: Some(0),
                attempt: 1
            }
        );
        assert!(sup.is_running(), "自动重启后应仍在运行");
        assert_eq!(sup.current_port(), Some(port), "重启应复用同端口");
        sup.stop();
    }

    #[tokio::test]
    async fn 就绪后静默运行不受就绪超时影响() {
        if !node_available() {
            return;
        }
        let port = crate::ports::pick_free_port().expect("空闲端口");
        // 打印就绪行后保持静默 1 秒再退出：就绪超时若仍计时，
        // 会在 300ms 处返回 Exited{code:None}（误杀）。
        let script = format!(
            "console.log('dsh web: http://127.0.0.1:{port}'); setTimeout(()=>process.exit(0), 1000)"
        );
        let mut sup = Supervisor::new(SupervisorOptions {
            ready_timeout: Duration::from_millis(300),
            max_attempts: 3,
            backoff: vec![Duration::from_millis(20)],
        });
        sup.start(port, "node", &["-e", &script], &[])
            .await
            .expect("start 成功");
        assert!(matches!(sup.wait().await, SupervisorEvent::Ready { .. }));
        // 应等到真实退出（约 1s 后，code 0），而不是 300ms 的超时误杀。
        let evt = sup.wait().await;
        assert_eq!(
            evt,
            SupervisorEvent::Exited {
                code: Some(0),
                attempt: 1
            },
            "就绪后不应被超时误杀"
        );
    }

    #[tokio::test]
    async fn 就绪超时按一次失败尝试处理() {
        if !node_available() {
            return;
        }
        let mut sup = Supervisor::new(SupervisorOptions {
            ready_timeout: Duration::from_millis(300),
            max_attempts: 3,
            backoff: vec![Duration::from_millis(20)],
        });
        let port = crate::ports::pick_free_port().expect("空闲端口");
        // 不打印就绪行，只是挂着。
        sup.start(port, "node", &["-e", "setInterval(()=>{},1000)"], &[])
            .await
            .expect("start 成功");
        let evt = sup.wait().await;
        assert_eq!(
            evt,
            SupervisorEvent::Exited {
                code: None,
                attempt: 1
            }
        );
        assert!(!sup.is_running(), "超时后应回收子进程");
    }
}
