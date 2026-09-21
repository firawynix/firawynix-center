; Ganchos do instalador NSIS que o Tauri gera (bundle.windows.nsis.installerHooks).
;
; Até a 1.0.x o launcher se chamava "Firawynix Games": outra pasta
; (%LOCALAPPDATA%\Firawynix Games), outra linha em Programas e Recursos, outros
; atalhos. Quem atualiza para o Firawynix Center não pode ficar com os dois — a
; versão velha sai aqui, sem janela e sem deixar rastro.

!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Firawynix Games" "UninstallString"
  ${If} $0 != ""
    ; _?= faz o desinstalador rodar no lugar e o ExecWait esperar de verdade
    ; (sem ele, o NSIS se copia para o TEMP e volta na hora)
    ExecWait '$0 /S _?=$LOCALAPPDATA\Firawynix Games'
    Delete "$LOCALAPPDATA\Firawynix Games\uninstall.exe"
    RMDir "$LOCALAPPDATA\Firawynix Games"
    Delete "$DESKTOP\Firawynix Games.lnk"
    Delete "$SMPROGRAMS\Firawynix Games.lnk"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Firawynix Games"
    DeleteRegKey HKCU "Software\Firawynix\Firawynix Games"
  ${EndIf}
!macroend
