//! Tudo o que fala com o Windows: registro (onde o jogo foi instalado), variáveis
//! de ambiente dos caminhos candidatos, ShellExecuteEx (que abre o jogo e os
//! instaladores pedindo UAC quando o manifesto do .exe exige administrador —
//! `std::process::Command` falharia com o erro 740 nesses casos), atalhos
//! (IShellLink) e a faxina da desinstalação completa.

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows::core::{w, Interface, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHGetKnownFolderPath, ShellExecuteExW, ShellLink, FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Documents,
    FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Programs, FOLDERID_PublicDesktop, FOLDERID_Videos,
    KF_FLAG_DEFAULT, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
use winreg::RegKey;

const DESINSTALAR: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
/// FILE_ATTRIBUTE_REPARSE_POINT: junção ou link — apagar nunca atravessa
const REPARSE: u32 = 0x400;

/// Um MSIX é atualizado pela Store ou pelo App Installer. Nesse ambiente o
/// atualizador NSIS/AppImage do Tauri não pode tentar substituir o executável.
pub fn executando_em_msix() -> bool {
    let mut tamanho = 0u32;
    unsafe { GetCurrentPackageFullName(&mut tamanho, None) == ERROR_INSUFFICIENT_BUFFER }
}

/// O que o desinstalador deixou em ...\Uninstall\<chave> (Inno, NSIS, MSI ou o
/// setup próprio do FolderPin/FirawSelector).
pub struct Registro {
    pub pasta: Option<PathBuf>,
    pub icone: Option<PathBuf>,
    pub versao: Option<String>,
    pub desinstalar: Option<String>,
    /// HKLM: instalado para todos — desinstalar pede administrador
    pub da_maquina: bool,
}

/// A chave vem do catálogo; mesmo validada no painel, não entra no caminho do
/// registro com barra ou qualquer coisa fora do formato de um AppId.
pub fn chave_valida(chave: &str) -> bool {
    !chave.is_empty()
        && chave.len() <= 120
        && chave.chars().all(|c| c.is_ascii_alphanumeric() || "{}_-. ".contains(c))
}

/// DisplayIcon vem como `"C:\x\jogo.exe",0` — tira as aspas e o índice.
fn limpar_icone(valor: &str) -> String {
    let mut v = valor.trim();
    if let Some(i) = v.rfind(',') {
        if v[i + 1..].trim().parse::<i32>().is_ok() {
            v = &v[..i];
        }
    }
    v.trim().trim_matches('"').to_string()
}

pub fn ler_registro(chave: &str) -> Option<Registro> {
    if !chave_valida(chave) {
        return None;
    }
    let caminho = format!(r"{DESINSTALAR}\{chave}");
    // Inno com PrivilegesRequired=lowest grava no HKCU; admin grava no HKLM
    // (na visão de 64 ou de 32 bits, conforme o instalador)
    let tentativas = [
        (HKEY_CURRENT_USER, KEY_READ, false),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY, true),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY, true),
    ];
    for (raiz, flags, da_maquina) in tentativas {
        let Ok(k) = RegKey::predef(raiz).open_subkey_with_flags(&caminho, flags) else {
            continue;
        };
        let texto = |nome: &str| {
            k.get_value::<String, _>(nome)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let icone = texto("DisplayIcon").map(|v| PathBuf::from(limpar_icone(&v)));
        let pasta = texto("InstallLocation")
            .map(|v| PathBuf::from(v.trim_matches('"')))
            .or_else(|| icone.as_ref().and_then(|i| i.parent().map(Path::to_path_buf)));
        return Some(Registro {
            pasta,
            icone,
            versao: texto("DisplayVersion"),
            desinstalar: texto("UninstallString"),
            da_maquina,
        });
    }
    None
}

/// Entrada de desinstalação que o próprio desinstalador esqueceu (só HKCU: a
/// do HKLM precisaria de administrador e o Inno sempre apaga a dele).
pub fn apagar_chave_desinstalar(chave: &str) -> bool {
    chave_valida(chave)
        && RegKey::predef(HKEY_CURRENT_USER)
            .delete_subkey_all(format!(r"{DESINSTALAR}\{chave}"))
            .is_ok()
}

/// Expande `%LOCALAPPDATA%\Jogo\jogo.exe`. Variável que não existe = caminho
/// descartado (melhor não achar do que achar no lugar errado).
pub fn expandir(caminho: &str) -> Option<String> {
    let mut saida = String::new();
    let mut resto = caminho;
    while let Some(ini) = resto.find('%') {
        saida.push_str(&resto[..ini]);
        let depois = &resto[ini + 1..];
        let fim = depois.find('%')?;
        let nome = &depois[..fim];
        if nome.is_empty() {
            saida.push('%');
        } else {
            saida.push_str(&std::env::var(nome).ok()?);
        }
        resto = &depois[fim + 1..];
    }
    saida.push_str(resto);
    Some(saida)
}

/// Executável relativo do catálogo: nada de `..`, unidade ou barra inicial.
pub fn relativo_seguro(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains("..")
        && !rel.starts_with(['\\', '/'])
        && !rel.contains(':')
        && rel.to_ascii_lowercase().ends_with(".exe")
}

pub fn e_exe(p: &Path) -> bool {
    p.is_file()
        && p.extension()
            .map_or(false, |e| e.to_string_lossy().eq_ignore_ascii_case("exe"))
}

/// O executável tem de estar DENTRO da pasta instalada, depois de resolver
/// links e `..` — o catálogo não consegue apontar para fora dela.
pub fn dentro(pasta: &Path, exe: &Path) -> bool {
    match (pasta.canonicalize(), exe.canonicalize()) {
        (Ok(p), Ok(e)) => e.starts_with(p),
        _ => false,
    }
}

/// `"C:\x\unins000.exe" /SILENT` -> (exe, args). Sem aspas, corta no ".exe".
pub fn separar_comando(cmd: &str) -> (String, Option<String>) {
    let c = cmd.trim();
    if let Some(resto) = c.strip_prefix('"') {
        if let Some(i) = resto.find('"') {
            let args = resto[i + 1..].trim();
            return (resto[..i].to_string(), (!args.is_empty()).then(|| args.to_string()));
        }
    }
    if let Some(i) = c.to_ascii_lowercase().find(".exe") {
        let fim = i + 4;
        let args = c[fim..].trim();
        return (c[..fim].to_string(), (!args.is_empty()).then(|| args.to_string()));
    }
    (c.to_string(), None)
}

/// Como rodar o desinstalador sem janela. Inno e MSI se reconhecem pelo próprio
/// comando (unins000.exe, msiexec); NSIS só pelo tipo do catálogo. O resto
/// (setup próprio) abre como sempre abriu.
pub struct ComandoDesinstalar {
    pub exe: PathBuf,
    pub args: Option<String>,
    pub oculto: bool,
}

pub fn comando_desinstalar(tipo: &str, comando: &str) -> ComandoDesinstalar {
    let (exe, args) = separar_comando(comando);
    let nome = Path::new(&exe)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if nome == "msiexec.exe" || nome == "msiexec" {
        let a = args.unwrap_or_default();
        // o registro guarda "/I{GUID}" (reparar); desinstalar é /X
        let a = match a.get(..2) {
            Some(p) if p.eq_ignore_ascii_case("/i") => format!("/X{}", &a[2..]),
            _ => a,
        };
        return ComandoDesinstalar { exe: PathBuf::from("msiexec.exe"), args: Some(format!("{a} /qn /norestart")), oculto: true };
    }
    // unins000.exe (Inno) — o uninstall.exe do NSIS também começa com "unins"
    let inno = nome
        .strip_prefix("unins")
        .and_then(|r| r.strip_suffix(".exe"))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if inno {
        return ComandoDesinstalar {
            exe: PathBuf::from(exe),
            args: Some("/VERYSILENT /SUPPRESSMSGBOXES /NORESTART".into()),
            oculto: true,
        };
    }
    if tipo == "nsis" {
        return ComandoDesinstalar { exe: PathBuf::from(exe), args: Some("/S".into()), oculto: true };
    }
    ComandoDesinstalar { exe: PathBuf::from(exe), args, oculto: false }
}

pub enum ErroExec {
    /// A pessoa disse "Não" no UAC.
    Cancelado,
    Outro(String),
}

/// Abre um arquivo/pasta/URL como se fosse um duplo clique. Com `esperar`,
/// bloqueia até o processo terminar e devolve o código de saída — chame numa
/// thread de bloqueio, nunca na do async runtime.
pub fn executar(alvo: &Path, args: Option<&str>, pasta: Option<&Path>, esperar: bool) -> Result<Option<u32>, ErroExec> {
    executar_com(alvo, args, pasta, esperar, false)
}

/// Instalador silencioso: nenhuma janela (o UAC, quando o instalador exige
/// administrador, continua aparecendo — é do Windows, não do instalador).
pub fn executar_oculto(alvo: &Path, args: Option<&str>) -> Result<Option<u32>, ErroExec> {
    executar_com(alvo, args, None, true, true)
}

fn executar_com(alvo: &Path, args: Option<&str>, pasta: Option<&Path>, esperar: bool, oculto: bool) -> Result<Option<u32>, ErroExec> {
    com();
    let alvo_w = HSTRING::from(alvo.as_os_str());
    let args_w = args.map(HSTRING::from);
    let pasta_w = pasta.map(|p| HSTRING::from(p.as_os_str()));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("open"),
        lpFile: PCWSTR(alvo_w.as_ptr()),
        lpParameters: args_w.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr())),
        lpDirectory: pasta_w.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
        nShow: if oculto { SW_HIDE.0 } else { SW_SHOWNORMAL.0 },
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| {
        if e.code() == ERROR_CANCELLED.to_hresult() {
            ErroExec::Cancelado
        } else {
            ErroExec::Outro(e.to_string())
        }
    })?;

    let processo = info.hProcess;
    if processo.is_invalid() {
        // abrir pasta/URL não devolve processo
        return Ok(None);
    }
    unsafe {
        if !esperar {
            let _ = CloseHandle(processo);
            return Ok(None);
        }
        WaitForSingleObject(processo, INFINITE);
        let mut codigo = 0u32;
        let _ = GetExitCodeProcess(processo, &mut codigo);
        let _ = CloseHandle(processo);
        Ok(Some(codigo))
    }
}

/// Espaço livre para o usuário atual no disco da pasta (que tem de existir).
pub fn espaco_livre(pasta: &Path) -> Option<u64> {
    let alvo = HSTRING::from(pasta.as_os_str());
    let mut livre = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(alvo.as_ptr()), Some(&mut livre as *mut u64), None, None) }.ok()?;
    Some(livre)
}

/// Bytes de tudo o que há na pasta (sem seguir links). É assim que a barra da
/// instalação silenciosa anda: o instalador não conta o progresso para ninguém.
pub fn tamanho_pasta(pasta: &Path) -> u64 {
    let mut total = 0u64;
    let mut pilha = vec![pasta.to_path_buf()];
    while let Some(atual) = pilha.pop() {
        let Ok(itens) = std::fs::read_dir(&atual) else { continue };
        for item in itens.flatten() {
            let Ok(meta) = item.path().symlink_metadata() else { continue };
            if meta.is_dir() {
                pilha.push(item.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

/// O que a janela de instalar decidiu sobre atalhos. O Inno recebe a escolha
/// (tarefa `desktopicon`), e o launcher confere depois.
#[derive(Clone, Copy)]
pub struct Tarefas {
    pub area: bool,
    pub menu: bool,
}

/// Parâmetros de instalação sem janela, por tipo de instalador. A pasta vem do
/// catálogo (caminho candidato, que o painel não deixa ter aspas) ou da janela
/// de escolher pasta; se viesse com aspas, fica de fora e o instalador usa a
/// pasta padrão dele.
pub fn argumentos_silenciosos(
    tipo: &str,
    instalador: &Path,
    pasta: Option<&Path>,
    log: &Path,
    tarefas: Option<Tarefas>,
) -> Option<(PathBuf, String)> {
    let pasta = pasta.map(|p| p.display().to_string()).filter(|p| !p.contains('"'));
    match tipo {
        "inno" => {
            let mut a = String::from("/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-");
            if let Some(p) = &pasta {
                a.push_str(&format!(" /DIR=\"{p}\""));
            }
            // atualização (tarefas = None): o Inno repete as escolhas da 1ª instalação
            if let Some(t) = tarefas {
                a.push_str(if t.area { " /MERGETASKS=\"desktopicon\"" } else { " /MERGETASKS=\"!desktopicon\"" });
                if !t.menu {
                    a.push_str(" /NOICONS");
                }
            }
            a.push_str(&format!(" /LOG=\"{}\"", log.display()));
            Some((instalador.to_path_buf(), a))
        }
        // NSIS exige o /D= por último e sem aspas
        "nsis" => Some((
            instalador.to_path_buf(),
            match &pasta {
                Some(p) => format!("/S /D={p}"),
                None => "/S".into(),
            },
        )),
        "msi" => Some((
            PathBuf::from("msiexec.exe"),
            format!("/i \"{}\" /qn /norestart /l*v \"{}\"", instalador.display(), log.display()),
        )),
        _ => None,
    }
}

/* ---------------- pastas do Windows ---------------- */

fn com() {
    // STA na thread atual; já iniciado (em qualquer modo) serve igual
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

pub fn pasta_conhecida(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let texto = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        texto.filter(|t| !t.is_empty()).map(PathBuf::from)
    }
}

/// Caminho comparável: canônico quando existe, minúsculo, sem `\\?\` nem barra no fim.
fn normal(p: &Path) -> String {
    let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let s = c.to_string_lossy().to_lowercase();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    s.trim_end_matches('\\').to_string()
}

fn contido(filho: &str, pai: &str) -> bool {
    filho == pai || filho.starts_with(&format!("{pai}\\"))
}

/* ---------------- atalhos ---------------- */

pub struct LocaisAtalho {
    pub area_usuario: Option<PathBuf>,
    pub area_todos: Option<PathBuf>,
    pub menu_usuario: Option<PathBuf>,
    pub menu_todos: Option<PathBuf>,
}

pub fn locais_atalho() -> LocaisAtalho {
    LocaisAtalho {
        area_usuario: pasta_conhecida(&FOLDERID_Desktop),
        area_todos: pasta_conhecida(&FOLDERID_PublicDesktop),
        menu_usuario: pasta_conhecida(&FOLDERID_Programs),
        menu_todos: pasta_conhecida(&FOLDERID_CommonPrograms),
    }
}

/// Nome do jogo como nome de arquivo (`Aion H4K.lnk`).
pub fn nome_de_arquivo(nome: &str) -> String {
    let n: String = nome
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let n = n.trim().trim_end_matches('.').trim().to_string();
    if n.is_empty() {
        "Jogo".into()
    } else {
        n
    }
}

pub fn criar_atalho(lnk: &Path, alvo: &Path, pasta: &Path, descricao: &str) -> Result<(), String> {
    com();
    let (alvo_w, pasta_w, desc_w, lnk_w) = (
        HSTRING::from(alvo.as_os_str()),
        HSTRING::from(pasta.as_os_str()),
        HSTRING::from(descricao),
        HSTRING::from(lnk.as_os_str()),
    );
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        link.SetPath(PCWSTR(alvo_w.as_ptr())).map_err(|e| e.to_string())?;
        link.SetWorkingDirectory(PCWSTR(pasta_w.as_ptr())).map_err(|e| e.to_string())?;
        link.SetIconLocation(PCWSTR(alvo_w.as_ptr()), 0).map_err(|e| e.to_string())?;
        link.SetDescription(PCWSTR(desc_w.as_ptr())).map_err(|e| e.to_string())?;
        let arquivo: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        if let Some(p) = lnk.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        arquivo.Save(PCWSTR(lnk_w.as_ptr()), true).map_err(|e| e.to_string())
    }
}

pub fn alvo_do_atalho(lnk: &Path) -> Option<PathBuf> {
    com();
    let lnk_w = HSTRING::from(lnk.as_os_str());
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let arquivo: IPersistFile = link.cast().ok()?;
        arquivo.Load(PCWSTR(lnk_w.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (n > 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..n])))
    }
}

fn lnks(dir: &Path, profundidade: u32, saida: &mut Vec<PathBuf>) {
    let Ok(itens) = std::fs::read_dir(dir) else { return };
    for item in itens.flatten() {
        let p = item.path();
        let Ok(meta) = p.symlink_metadata() else { continue };
        if meta.is_dir() {
            if profundidade > 0 && meta.file_attributes() & REPARSE == 0 {
                lnks(&p, profundidade - 1, saida);
            }
        } else if p.extension().map_or(false, |e| e.eq_ignore_ascii_case("lnk")) {
            saida.push(p);
        }
    }
}

pub struct Atalho {
    pub caminho: PathBuf,
    pub area: bool,
    pub de_todos: bool,
}

/// Atalhos (área de trabalho e menu Iniciar, do usuário e de todos) que apontam
/// para dentro da pasta do jogo — os do instalador e os do launcher.
pub fn atalhos_para(pasta: &Path) -> Vec<Atalho> {
    let base = normal(pasta);
    if base.len() < 4 {
        return vec![];
    }
    let l = locais_atalho();
    let mut achados = Vec::new();
    for (dir, area, de_todos) in [
        (l.area_usuario, true, false),
        (l.area_todos, true, true),
        (l.menu_usuario, false, false),
        (l.menu_todos, false, true),
    ] {
        let Some(dir) = dir else { continue };
        let mut lista = Vec::new();
        // menu: o instalador costuma criar uma subpasta com o nome do jogo
        lnks(&dir, if area { 0 } else { 1 }, &mut lista);
        for lnk in lista {
            if alvo_do_atalho(&lnk).is_some_and(|alvo| contido(&normal(&alvo), &base)) {
                achados.push(Atalho { caminho: lnk, area, de_todos });
            }
        }
    }
    achados
}

/// Deixa os atalhos como a pessoa escolheu: cria o que falta (apontando para o
/// executável do catálogo — o launcher do próprio jogo, que se atualiza mesmo
/// sem o Firawynix Center) e tira o que ela não quis. Devolve os que criou.
pub fn ajustar_atalhos(nome: &str, exe: &Path, pasta: &Path, area: bool, menu: bool) -> Vec<PathBuf> {
    let existentes = atalhos_para(pasta);
    let l = locais_atalho();
    let mut criados = Vec::new();
    for (quero, e_area, destino) in [(area, true, l.area_usuario), (menu, false, l.menu_usuario)] {
        let deste: Vec<&Atalho> = existentes.iter().filter(|a| a.area == e_area).collect();
        if quero {
            if deste.is_empty() {
                if let Some(d) = destino {
                    let lnk = d.join(format!("{nome}.lnk"));
                    if criar_atalho(&lnk, exe, pasta, nome).is_ok() {
                        criados.push(lnk);
                    }
                }
            }
        } else {
            for a in deste {
                apagar_atalho(&a.caminho);
            }
        }
    }
    criados
}

/// Apaga o atalho e, no menu Iniciar, a subpasta que ficou vazia.
pub fn apagar_atalho(lnk: &Path) -> bool {
    if std::fs::remove_file(lnk).is_err() {
        return !lnk.exists();
    }
    if let Some(pai) = lnk.parent() {
        let l = locais_atalho();
        let raiz_menu = [l.menu_usuario, l.menu_todos].into_iter().flatten().map(|p| normal(&p)).any(|m| {
            let p = normal(pai);
            p != m && contido(&p, &m)
        });
        if raiz_menu {
            let _ = std::fs::remove_dir(pai); // só sai se estiver vazia
        }
    }
    true
}

/* ---------------- apagar pasta com segurança ---------------- */

fn protegidas() -> Vec<String> {
    let mut v: Vec<PathBuf> = [
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "SystemRoot",
        "windir",
        "PUBLIC",
        "TEMP",
        "TMP",
        "OneDrive",
    ]
    .iter()
    .filter_map(|n| std::env::var(n).ok())
    .map(PathBuf::from)
    .collect();
    for id in [
        FOLDERID_Desktop,
        FOLDERID_PublicDesktop,
        FOLDERID_Documents,
        FOLDERID_Downloads,
        FOLDERID_Pictures,
        FOLDERID_Videos,
        FOLDERID_Music,
        FOLDERID_Programs,
        FOLDERID_CommonPrograms,
    ] {
        v.extend(pasta_conhecida(&id));
    }
    if let Ok(d) = std::env::var("SystemDrive") {
        v.push(PathBuf::from(format!(r"{d}\Games")));
    }
    v.iter().map(|p| normal(p)).collect()
}

/// A desinstalação completa só apaga pasta que não é do sistema nem contém uma:
/// `C:\Games\Aion H4K` pode, `C:\Games`, a área de trabalho ou o perfil não.
pub fn pasta_apagavel(p: &Path) -> Result<(), String> {
    let meta = p.symlink_metadata().map_err(|_| "a pasta não existe mais".to_string())?;
    if !meta.is_dir() {
        return Err("não é uma pasta".into());
    }
    if meta.file_attributes() & REPARSE != 0 {
        return Err("é um link para outra pasta".into());
    }
    let n = normal(p);
    let partes: Vec<&str> = n.split('\\').filter(|s| !s.is_empty()).collect();
    // unidade local + ao menos duas pastas (C:\Games\Jogo)
    if partes.len() < 3 || !partes[0].ends_with(':') || partes[0].len() != 2 {
        return Err("fica perto demais da raiz do disco".into());
    }
    if protegidas().iter().any(|prot| contido(prot, &n)) {
        return Err("é uma pasta do Windows ou tem uma dentro".into());
    }
    Ok(())
}

/// Apaga a árvore sem atravessar junções/links (o link sai, o destino fica).
/// Devolve quantos itens não saíram (arquivo em uso, sem permissão).
pub fn apagar_arvore(p: &Path) -> usize {
    let mut falhas = 0;
    if let Ok(itens) = std::fs::read_dir(p) {
        for item in itens.flatten() {
            let caminho = item.path();
            let Ok(meta) = caminho.symlink_metadata() else {
                falhas += 1;
                continue;
            };
            let link = meta.file_attributes() & REPARSE != 0;
            if meta.is_dir() && !link {
                falhas += apagar_arvore(&caminho);
            } else if meta.is_dir() {
                if std::fs::remove_dir(&caminho).is_err() {
                    falhas += 1;
                }
            } else {
                if meta.permissions().readonly() {
                    let mut perm = meta.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(&caminho, perm);
                }
                if std::fs::remove_file(&caminho).is_err() {
                    falhas += 1;
                }
            }
        }
    }
    if std::fs::remove_dir(p).is_err() && falhas == 0 && p.exists() {
        falhas += 1;
    }
    falhas
}

/* ---------------- faxina da desinstalação completa ---------------- */

pub enum Limpeza {
    Pasta(PathBuf),
    /// subchave do HKCU, ex.: `Software\Webzen\Mu`
    Registro(String),
}

const RAIZES_LIMPEZA: [&str; 5] = [
    "%APPDATA%\\",
    "%LOCALAPPDATA%\\",
    "%PROGRAMDATA%\\",
    "%USERPROFILE%\\Documents\\",
    "%SystemDrive%\\Games\\",
];

/// Primeiro nível que nunca é de um jogo — nem por engano de digitação no painel.
const DO_SISTEMA: [&str; 16] = [
    "microsoft",
    "windows",
    "packages",
    "temp",
    "programs",
    "classes",
    "policies",
    "wow6432node",
    "clients",
    "registeredapplications",
    "local",
    "locallow",
    "roaming",
    "google",
    "mozilla",
    "firawynix center",
];

/// Entrada do catálogo -> o que apagar. Caminho só debaixo das raízes acima e
/// com uma pasta própria depois delas; registro só `HKCU\Software\X\Y`.
pub fn limpeza(entrada: &str) -> Option<Limpeza> {
    let e = entrada.trim();
    if e.is_empty() || e.len() > 260 || e.contains("..") || e.contains('/') || e.chars().any(|c| "*?\"<>|".contains(c)) {
        return None;
    }
    let baixo = e.to_ascii_lowercase();
    const REG: &str = "hkcu\\software\\";
    if baixo.starts_with(REG) {
        let partes: Vec<&str> = e[REG.len()..].split('\\').collect();
        if partes.len() < 2
            || partes.iter().any(|p| p.trim().is_empty())
            || DO_SISTEMA.contains(&partes[0].to_ascii_lowercase().as_str())
        {
            return None;
        }
        return Some(Limpeza::Registro(format!("Software\\{}", partes.join("\\"))));
    }
    let raiz = RAIZES_LIMPEZA.iter().find(|r| baixo.starts_with(&r.to_ascii_lowercase()))?;
    let primeiro = e[raiz.len()..].split('\\').next().unwrap_or("").trim();
    if primeiro.is_empty() || DO_SISTEMA.contains(&primeiro.to_ascii_lowercase().as_str()) {
        return None;
    }
    Some(Limpeza::Pasta(PathBuf::from(expandir(e)?)))
}

impl Limpeza {
    pub fn existe(&self) -> bool {
        match self {
            Limpeza::Pasta(p) => p.symlink_metadata().is_ok(),
            Limpeza::Registro(k) => RegKey::predef(HKEY_CURRENT_USER).open_subkey(k).is_ok(),
        }
    }

    pub fn descricao(&self) -> String {
        match self {
            Limpeza::Pasta(p) => p.display().to_string(),
            Limpeza::Registro(k) => format!(r"Registro: HKCU\{k}"),
        }
    }

    pub fn aplicar(&self) -> Result<(), String> {
        match self {
            Limpeza::Pasta(p) => {
                let Ok(meta) = p.symlink_metadata() else { return Ok(()) };
                if meta.is_dir() {
                    pasta_apagavel(p)?;
                    match apagar_arvore(p) {
                        0 => Ok(()),
                        n => Err(format!("{n} item(ns) em uso")),
                    }
                } else {
                    std::fs::remove_file(p).map_err(|e| e.to_string())
                }
            }
            Limpeza::Registro(k) => {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                if hkcu.open_subkey(k).is_err() {
                    return Ok(());
                }
                hkcu.delete_subkey_all(k).map_err(|e| e.to_string())?;
                // a de cima (Software\Webzen) vazia vai junto — nunca a própria Software
                if let Some((pai, _)) = k.rsplit_once('\\') {
                    if pai.contains('\\') {
                        let vazia = hkcu
                            .open_subkey(pai)
                            .map_or(false, |c| c.enum_keys().next().is_none() && c.enum_values().next().is_none());
                        if vazia {
                            let _ = hkcu.delete_subkey(pai);
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn argumentos_por_tipo() {
        let inst = Path::new(r"C:\t\mu-setup.exe");
        let log = Path::new(r"C:\t\mu.log");
        let (alvo, a) = argumentos_silenciosos("inno", inst, Some(Path::new(r"C:\Games\Firawynix MU")), log, None).unwrap();
        assert_eq!(alvo, inst);
        assert!(a.starts_with("/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-"));
        assert!(a.contains(r#"/DIR="C:\Games\Firawynix MU""#));
        assert!(!a.contains("MERGETASKS"));
        let (_, a) = argumentos_silenciosos("inno", inst, None, log, Some(Tarefas { area: false, menu: false })).unwrap();
        assert!(a.contains(r#"/MERGETASKS="!desktopicon""#) && a.contains("/NOICONS"));
        let (_, a) = argumentos_silenciosos("inno", inst, None, log, Some(Tarefas { area: true, menu: true })).unwrap();
        assert!(a.contains(r#"/MERGETASKS="desktopicon""#) && !a.contains("/NOICONS"));
        let (_, a) = argumentos_silenciosos("nsis", inst, Some(Path::new(r"C:\Jogo X")), log, None).unwrap();
        assert_eq!(a, r"/S /D=C:\Jogo X");
        let (alvo, _) = argumentos_silenciosos("msi", inst, None, log, None).unwrap();
        assert_eq!(alvo, Path::new("msiexec.exe"));
        assert!(argumentos_silenciosos("manual", inst, None, log, None).is_none());
    }

    #[test]
    fn desinstalar_sem_janela() {
        let c = comando_desinstalar("inno", r#""C:\Games\Firawynix MU\unins000.exe""#);
        assert_eq!(c.exe, Path::new(r"C:\Games\Firawynix MU\unins000.exe"));
        assert_eq!(c.args.as_deref(), Some("/VERYSILENT /SUPPRESSMSGBOXES /NORESTART"));
        assert!(c.oculto);
        let c = comando_desinstalar("msi", "MsiExec.exe /I{ABC}");
        assert_eq!(c.args.as_deref(), Some("/X{ABC} /qn /norestart"));
        let c = comando_desinstalar("nsis", r#""C:\x\uninstall.exe""#);
        assert_eq!(c.args.as_deref(), Some("/S"));
        // setup próprio (FolderPin): abre como sempre
        let c = comando_desinstalar("manual", r#""C:\x\Desinstalar.exe" --uninstall"#);
        assert!(!c.oculto);
        assert_eq!(c.args.as_deref(), Some("--uninstall"));
    }

    #[test]
    fn mede_pasta() {
        let d = std::env::temp_dir().join(format!("fw-teste-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(d.join("sub").join("b.bin"), vec![0u8; 234]).unwrap();
        assert_eq!(tamanho_pasta(&d), 1234);
        assert_eq!(tamanho_pasta(&d.join("nao-existe")), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn apaga_so_pasta_de_jogo() {
        assert!(pasta_apagavel(Path::new(r"C:\")).is_err());
        assert!(pasta_apagavel(&std::env::temp_dir()).is_err());
        assert!(pasta_apagavel(Path::new(&std::env::var("USERPROFILE").unwrap())).is_err());
        assert!(pasta_apagavel(Path::new(&std::env::var("LOCALAPPDATA").unwrap())).is_err());
        if let Some(area) = pasta_conhecida(&FOLDERID_Desktop) {
            assert!(pasta_apagavel(&area).is_err());
        }
        // pasta de jogo de verdade, com arquivo somente leitura dentro
        let d = std::env::temp_dir().join(format!("fw-jogo-{}", std::process::id()));
        std::fs::create_dir_all(d.join("bin64")).unwrap();
        let f = d.join("bin64").join("aion.bin");
        std::fs::write(&f, b"x").unwrap();
        let mut perm = std::fs::metadata(&f).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&f, perm).unwrap();
        assert!(pasta_apagavel(&d).is_ok());
        assert_eq!(apagar_arvore(&d), 0);
        assert!(!d.exists());
    }

    #[test]
    fn limpeza_so_no_que_e_do_jogo() {
        assert!(matches!(limpeza(r"HKCU\Software\Webzen\Mu"), Some(Limpeza::Registro(k)) if k == r"Software\Webzen\Mu"));
        assert!(matches!(limpeza(r"%LOCALAPPDATA%\Aion-H4K-Launcher"), Some(Limpeza::Pasta(_))));
        assert!(limpeza(r"HKCU\Software\Webzen").is_none()); // só um nível: largo demais
        assert!(limpeza(r"HKCU\Software\Microsoft\Windows").is_none());
        assert!(limpeza(r"HKLM\Software\Jogo\X").is_none());
        assert!(limpeza(r"%APPDATA%\Microsoft").is_none());
        assert!(limpeza(r"%APPDATA%\").is_none());
        assert!(limpeza(r"%LOCALAPPDATA%\..\x").is_none());
        assert!(limpeza(r"C:\Windows").is_none());
        assert!(limpeza(r"%USERPROFILE%\Desktop\x").is_none());
        assert!(limpeza(r"%APPDATA%\Jogo\*").is_none());
    }

    #[test]
    fn nome_de_atalho() {
        assert_eq!(nome_de_arquivo("Aion H4K"), "Aion H4K");
        assert_eq!(nome_de_arquivo("ERP + Caixa (PDV)"), "ERP + Caixa (PDV)");
        assert_eq!(nome_de_arquivo(r#"A/B:C*"#), "A B C");
        assert_eq!(nome_de_arquivo("..."), "Jogo");
    }

    #[test]
    fn chave_so_no_formato_de_appid() {
        assert!(chave_valida("{7A3F1B7C-52D4-4B8E-9C1A-8E2D5F0A6B31}_is1"));
        assert!(chave_valida("Tibia_is1"));
        assert!(chave_valida("FolderPin"));
        assert!(!chave_valida(r"..\..\Run"));
        assert!(!chave_valida(""));
    }

    #[test]
    fn relativo_nao_sai_da_pasta() {
        assert!(relativo_seguro("FirawynixLauncher.exe"));
        assert!(relativo_seguro(r"bin64\aion.exe"));
        assert!(relativo_seguro("FolderPin Studio.exe"));
        assert!(!relativo_seguro(r"..\cmd.exe"));
        assert!(!relativo_seguro(r"C:\Windows\cmd.exe"));
        assert!(!relativo_seguro(r"\Windows\cmd.exe"));
        assert!(!relativo_seguro("leiame.txt"));
    }

    #[test]
    fn limpa_display_icon() {
        assert_eq!(limpar_icone(r#""C:\Games\MU\x.exe",0"#), r"C:\Games\MU\x.exe");
        assert_eq!(limpar_icone(r"C:\Games\MU\x.exe"), r"C:\Games\MU\x.exe");
    }

    #[test]
    fn separa_comando_de_desinstalar() {
        let (exe, args) = separar_comando(r#""C:\Games\Firawynix MU\unins000.exe""#);
        assert_eq!(exe, r"C:\Games\Firawynix MU\unins000.exe");
        assert!(args.is_none());
        let (exe, args) = separar_comando("MsiExec.exe /X{ABC}");
        assert_eq!(exe, "MsiExec.exe");
        assert_eq!(args.as_deref(), Some("/X{ABC}"));
    }

    #[test]
    fn expande_variaveis() {
        std::env::set_var("FW_TESTE_PASTA", r"C:\X");
        assert_eq!(expandir(r"%FW_TESTE_PASTA%\a.exe").as_deref(), Some(r"C:\X\a.exe"));
        assert!(expandir(r"%FW_NAO_EXISTE_123%\a.exe").is_none());
    }

    /// Ciclo inteiro com um instalador Inno de verdade (por usuário, sem UAC):
    /// `FW_TESTE_INNO=<setup.exe> cargo test -- --ignored ciclo_inno`
    #[test]
    #[ignore]
    fn ciclo_inno() {
        use std::time::{Duration, Instant};
        let setup = std::env::var("FW_TESTE_INNO").expect("FW_TESTE_INNO com o caminho do setup de teste");
        let chave = "{5B7D6E2A-1C3F-4A8B-9D0E-7F6A5B4C3D21}_is1";
        let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap());
        let pasta = local.join("FirawynixTeste");
        let log = std::env::temp_dir().join("fw-teste-inno.log");

        // 1) instala sem janela: sem atalho na área de trabalho, com menu Iniciar
        let (alvo, args) =
            argumentos_silenciosos("inno", Path::new(&setup), Some(&pasta), &log, Some(Tarefas { area: false, menu: true })).unwrap();
        assert!(matches!(executar_oculto(&alvo, Some(&args)), Ok(Some(0))));
        let reg = ler_registro(chave).expect("o Inno grava a chave de desinstalação");
        let exe = pasta.join("TesteLauncher.exe");
        assert!(e_exe(&exe));
        let antes = atalhos_para(&pasta);
        assert!(antes.iter().all(|a| !a.area), "/MERGETASKS=!desktopicon devia impedir o atalho da área de trabalho");
        assert_eq!(antes.len(), 2, "o grupo do menu Iniciar tem o jogo e o desinstalador");

        // 2) o launcher ajusta: quer área de trabalho, não quer menu
        let criados = ajustar_atalhos("Firawynix Teste", &exe, &pasta, true, false);
        assert_eq!(criados.len(), 1);
        let depois = atalhos_para(&pasta);
        assert!(depois.len() == 1 && depois[0].area, "sobrou atalho no menu Iniciar");
        let grupo = locais_atalho().menu_usuario.unwrap().join("Firawynix Teste");
        assert!(!grupo.exists(), "a pasta vazia do menu devia sair junto");

        // 3) sobras que o jogo criaria rodando
        std::fs::write(pasta.join("config-do-jogo.ini"), b"x").unwrap();
        let dados = local.join("FwTesteDados");
        std::fs::create_dir_all(dados.join("cache")).unwrap();

        // 4) desinstala sem janela e espera o Inno terminar
        let cmd = comando_desinstalar("inno", reg.desinstalar.as_deref().unwrap());
        assert!(cmd.oculto);
        assert!(matches!(executar_oculto(&cmd.exe, cmd.args.as_deref()), Ok(Some(0))));
        let limite = Instant::now() + Duration::from_secs(60);
        while ler_registro(chave).is_some() && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(300));
        }
        assert!(ler_registro(chave).is_none());
        assert!(pasta.join("config-do-jogo.ini").exists(), "o Inno só leva o que ele instalou");

        // 5) remoção completa: atalho do launcher, resto da pasta, dados e registro
        for a in &criados {
            assert!(apagar_atalho(a));
        }
        pasta_apagavel(&pasta).unwrap();
        assert_eq!(apagar_arvore(&pasta), 0);
        for e in [r"%LOCALAPPDATA%\FwTesteDados", r"HKCU\Software\FwTeste\Jogo"] {
            let item = limpeza(e).unwrap();
            assert!(item.existe(), "{e} devia existir antes");
            item.aplicar().unwrap();
            assert!(!item.existe(), "{e} devia sair");
        }
        assert!(
            RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\FwTeste").is_err(),
            "a chave de cima, vazia, devia sair junto"
        );
        assert!(!pasta.exists() && !dados.exists());
        assert!(atalhos_para(&pasta).is_empty());
    }

    #[test]
    fn cria_e_le_atalho() {
        let d = std::env::temp_dir().join(format!("fw-lnk-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let alvo = std::env::current_exe().unwrap();
        let lnk = d.join("Teste.lnk");
        criar_atalho(&lnk, &alvo, &d, "teste").unwrap();
        assert_eq!(normal(&alvo_do_atalho(&lnk).unwrap()), normal(&alvo));
        let _ = std::fs::remove_dir_all(&d);
    }
}
