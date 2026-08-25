; 方案 A：安装包只携带 sidecar-dist.tar + sidecar-version.json（不再平铺
; sidecar-dist/ 目录）。这两个宏清理旧版安装遗留的解压目录，保证升级/卸载后
; 安装目录里不残留 300MB 的孤儿 node_modules。
!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$INSTDIR\sidecar-dist"
    RMDir /r "$INSTDIR\sidecar-dist"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} ${FileExists} "$INSTDIR\sidecar-dist"
    RMDir /r "$INSTDIR\sidecar-dist"
  ${EndIf}
!macroend
