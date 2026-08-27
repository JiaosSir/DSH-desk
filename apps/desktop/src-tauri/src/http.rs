//! WinHTTP 极简客户端：走系统组件 winhttp.dll（自动跟随系统代理与根证书），
//! 零新依赖。同步模式，调用方放在 spawn_blocking 里执行。
//! 场景：查询 GitHub Releases API（get_text）与下载安装包（download）。

use std::ffi::c_void;
use std::io::Write;
use std::path::Path;
use std::ptr;

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::*;

/// 响应体上限（检查响应为 release JSON，远小于此）。
const MAX_BODY: usize = 1 << 20;
/// 读缓冲（下载分块）。
const CHUNK: usize = 64 * 1024;

/// WinHTTP 句柄（RAII 关闭）。
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn err(what: &str, e: windows::core::Error) -> String {
    format!("{what}（WinHTTP 错误 0x{:08X}）", e.code().0 as u32)
}

/// 响应体分块回调：(chunk, content_length)。
type DataSink<'a> = dyn FnMut(&[u8], u64) -> Result<(), String> + 'a;

/// 发起 GET 请求并把响应体分块交给 `on_data`；返回 Content-Length（未知为 0）。
/// 自动跟随重定向（GitHub 资产 URL 会 302 到 objects.githubusercontent.com）。
fn request(
    url: &str,
    user_agent: &str,
    receive_timeout_ms: u32,
    on_data: &mut DataSink<'_>,
) -> Result<u64, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("URL 无效: {e}"))?;
    let host = parsed.host_str().ok_or_else(|| "URL 缺少主机".to_owned())?;
    let secure = parsed.scheme() == "https";
    let port = parsed
        .port_or_known_default()
        .unwrap_or(if secure { 443 } else { 80 });
    let path = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };
    let full_path = match parsed.query() {
        Some(q) => format!("{path}?{q}"),
        None => path.to_owned(),
    };

    // 会话（自动代理：跟随系统代理设置）。
    let ua = w(user_agent);
    let session = Handle(unsafe {
        WinHttpOpen(
            PCWSTR(ua.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err("初始化网络失败（WinHttpOpen）".to_owned());
    }

    let host_w = w(host);
    let connect = Handle(unsafe { WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), port, 0) });
    if connect.0.is_null() {
        return Err("连接失败（WinHttpConnect）".to_owned());
    }

    let request = Handle(unsafe {
        WinHttpOpenRequest(
            connect.0,
            PCWSTR(w("GET").as_ptr()),
            PCWSTR(w(&full_path).as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            ptr::null(),
            if secure {
                WINHTTP_FLAG_SECURE
            } else {
                WINHTTP_OPEN_REQUEST_FLAGS(0)
            },
        )
    });
    if request.0.is_null() {
        return Err("创建请求失败（WinHttpOpenRequest）".to_owned());
    }

    unsafe {
        // 显式允许跨主机重定向（默认策略对 GET 也可用，这里钉死为 ALWAYS）。
        WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS.to_le_bytes()),
        )
        .map_err(|e| err("设置重定向失败", e))?;
        // 解析/连接 10s，发送 10s，接收按调用方（下载给更宽裕的 60s）。
        WinHttpSetTimeouts(request.0, 10_000, 10_000, 10_000, receive_timeout_ms as i32)
            .map_err(|e| err("设置超时失败", e))?;
        let headers = w(&format!(
            "User-Agent: {user_agent}\r\nAccept: application/vnd.github+json\r\n"
        ));
        // w() 尾部含 NUL 终止符；WinHttpSendRequest 的 dwHeadersLength 按「不含
        // 终止符」计——把长度参数一并计入会把 NUL 混进头部文本，触发
        // ERROR_INVALID_PARAMETER（0x80070057）。切片去掉 NUL，指针不变。
        WinHttpSendRequest(
            request.0,
            Some(&headers[..headers.len() - 1]),
            None,
            0,
            0,
            0,
        )
        .map_err(|e| err("发送请求失败", e))?;
        WinHttpReceiveResponse(request.0, ptr::null_mut()).map_err(|e| err("接收响应失败", e))?;
    }

    // 状态码。
    let mut status: u32 = 0;
    let mut len: u32 = std::mem::size_of::<u32>() as u32;
    let queried = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            ptr::null_mut(),
        )
    };
    if queried.is_err() {
        return Err("无法读取响应状态".to_owned());
    }
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }

    // Content-Length（chunked/未知时为 0）。
    let mut content_len: u32 = 0;
    let mut len: u32 = std::mem::size_of::<u32>() as u32;
    let total = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut content_len as *mut u32 as *mut c_void),
            &mut len,
            ptr::null_mut(),
        )
    }
    .map(|_| content_len as u64)
    .unwrap_or(0);

    // 分块读取。
    let mut buf = vec![0u8; CHUNK];
    loop {
        let mut available: u32 = 0;
        unsafe {
            WinHttpQueryDataAvailable(request.0, &mut available)
                .map_err(|e| err("读取响应失败", e))?;
        }
        if available == 0 {
            break;
        }
        let mut read: u32 = 0;
        let want = available.min(buf.len() as u32);
        unsafe {
            WinHttpReadData(request.0, buf.as_mut_ptr() as *mut c_void, want, &mut read)
                .map_err(|e| err("读取响应失败", e))?;
        }
        if read == 0 {
            break;
        }
        on_data(&buf[..read as usize], total)?;
    }
    Ok(total)
}

/// GET 文本响应（上限 1MB，防异常放大）。
pub fn get_text(url: &str, user_agent: &str, receive_timeout_ms: u32) -> Result<String, String> {
    let mut body: Vec<u8> = Vec::new();
    request(url, user_agent, receive_timeout_ms, &mut |chunk, _| {
        if body.len() + chunk.len() > MAX_BODY {
            return Err("响应体过大".to_owned());
        }
        body.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// 下载到 `dest`；进度经 `on_progress(received, total)` 回调（total 未知时为 0）。
pub fn download(
    url: &str,
    user_agent: &str,
    dest: &Path,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<u64, String> {
    let mut file = std::fs::File::create(dest).map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut received: u64 = 0;
    let total = request(url, user_agent, 60_000, &mut |chunk, total| {
        file.write_all(chunk)
            .map_err(|e| format!("写入安装包失败: {e}"))?;
        received += chunk.len() as u64;
        on_progress(received, total);
        Ok(())
    })?;
    file.flush().map_err(|e| format!("写入安装包失败: {e}"))?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    /// 起一个 127.0.0.1 微型 HTTP 应答：固定状态与 body（带 Content-Length）。
    /// 返回 (url, 线程句柄)；15s 无人连接则自行退出，避免挂死测试。
    fn serve_once(status: String, body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定回环端口");
        let addr = listener.local_addr().expect("读端口");
        listener.set_nonblocking(true).expect("非阻塞");
        let handle = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0u8; 4096];
                        let _ = stream.read(&mut buf);
                        let head = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(&body);
                        return;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() > deadline {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => return,
                }
            }
        });
        (format!("http://{addr}/test"), handle)
    }

    #[test]
    fn get_text_走通本地回环() {
        let (url, handle) = serve_once("200 OK".into(), b"hello dsh".to_vec());
        let text = get_text(&url, "dsh-desk-test", 10_000).expect("应成功");
        assert_eq!(text, "hello dsh");
        handle.join().expect("服务线程");
    }

    #[test]
    fn get_text_非200报错() {
        let (url, handle) = serve_once("404 Not Found".into(), b"nope".to_vec());
        let err = get_text(&url, "dsh-desk-test", 10_000).expect_err("应报 HTTP 404");
        assert!(err.contains("HTTP 404"), "错误信息: {err}");
        handle.join().expect("服务线程");
    }

    #[test]
    fn download_进度与落盘一致() {
        let body = vec![b'x'; 256 * 1024 + 17];
        let (url, handle) = serve_once("200 OK".into(), body.clone());
        let dest = std::env::temp_dir().join("dsh-http-test.bin");
        let mut last = (0u64, 0u64);
        let total = download(&url, "dsh-desk-test", &dest, &mut |received, total| {
            last = (received, total);
        })
        .expect("应成功");
        assert_eq!(total, body.len() as u64);
        assert_eq!(last, (body.len() as u64, body.len() as u64));
        let written = std::fs::read(&dest).expect("读落盘文件");
        assert_eq!(written.len(), body.len());
        assert_eq!(written, body);
        let _ = std::fs::remove_file(&dest);
        handle.join().expect("服务线程");
    }
}
