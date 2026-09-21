//! Integração nativa do Firawynix Center com desktops Linux.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn executando_em_msix() -> bool {
    false
}
use std::process::{Command, Stdio};

pub struct Registro {
    pub pasta: Option<PathBuf>,
    pub icone: Option<PathBuf>,
    pub versao: Option<String>,
    pub desinstalar: Option<String>,
    pub da_maquina: bool,
}

pub fn ler_registro(_chave: &str) -> Option<Registro> {
    None
}

pub fn apagar_chave_desinstalar(_chave: &str) -> bool {
    true
}

/// Expande a notação `%HOME%/...` usada pelo catálogo sem invocar um shell.
pub fn expandir(caminho: &str) -> Option<String> {
    let mut saida = String::new();
    let mut resto = caminho;
    while let Some(ini) = resto.find('%') {
        saida.push_str(&resto[..ini]);
        let depois = &resto[ini + 1..];
        let fim = depois.find('%')?;
        let nome = &depois[..fim];
        let valor = match nome {
            "XDG_DATA_HOME" => std::env::var("XDG_DATA_HOME").ok().or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| format!("{h}/.local/share"))
            }),
            "XDG_CONFIG_HOME" => std::env::var("XDG_CONFIG_HOME")
                .ok()
                .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.config"))),
            _ => std::env::var(nome).ok(),
        }?;
        saida.push_str(&valor);
        resto = &depois[fim + 1..];
    }
    saida.push_str(resto);
    Some(saida)
}

pub fn relativo_seguro(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() <= 200
        && !rel.contains("..")
        && !rel.starts_with('/')
        && !rel.contains('\0')
}

pub fn e_exe(p: &Path) -> bool {
    p.is_file()
        && p.metadata()
            .map_or(false, |m| m.permissions().mode() & 0o111 != 0)
}

pub fn dentro(pasta: &Path, exe: &Path) -> bool {
    match (pasta.canonicalize(), exe.canonicalize()) {
        (Ok(p), Ok(e)) => e.starts_with(p),
        _ => false,
    }
}

pub struct ComandoDesinstalar {
    pub exe: PathBuf,
    pub args: Option<String>,
    pub oculto: bool,
}

pub fn comando_desinstalar(_tipo: &str, comando: &str) -> ComandoDesinstalar {
    ComandoDesinstalar {
        exe: PathBuf::from(comando),
        args: None,
        oculto: false,
    }
}

pub enum ErroExec {
    Cancelado,
    Outro(String),
}

fn executar_com(
    alvo: &Path,
    args: Option<&str>,
    pasta: Option<&Path>,
    esperar: bool,
    oculto: bool,
) -> Result<Option<u32>, ErroExec> {
    let texto = alvo.to_string_lossy();
    let abrir_externamente = texto.starts_with("https://") || alvo.is_dir();
    let mut comando = if abrir_externamente {
        let mut c = Command::new("xdg-open");
        c.arg(alvo);
        c
    } else {
        let mut c = Command::new(alvo);
        if let Some(a) = args {
            c.args(a.split_whitespace());
        }
        c
    };
    if let Some(p) = pasta {
        comando.current_dir(p);
    }
    if oculto {
        comando.stdout(Stdio::null()).stderr(Stdio::null());
    }
    if esperar {
        let status = comando
            .status()
            .map_err(|e| ErroExec::Outro(e.to_string()))?;
        Ok(status.code().map(|c| c as u32))
    } else {
        comando
            .spawn()
            .map_err(|e| ErroExec::Outro(e.to_string()))?;
        Ok(None)
    }
}

pub fn executar(
    alvo: &Path,
    args: Option<&str>,
    pasta: Option<&Path>,
    esperar: bool,
) -> Result<Option<u32>, ErroExec> {
    executar_com(alvo, args, pasta, esperar, false)
}

pub fn executar_oculto(alvo: &Path, args: Option<&str>) -> Result<Option<u32>, ErroExec> {
    executar_com(alvo, args, None, true, true)
}

pub fn espaco_livre(pasta: &Path) -> Option<u64> {
    let caminho = CString::new(pasta.as_os_str().as_bytes()).ok()?;
    let mut dados = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(caminho.as_ptr(), dados.as_mut_ptr()) } != 0 {
        return None;
    }
    let dados = unsafe { dados.assume_init() };
    Some((dados.f_bavail as u64).saturating_mul(dados.f_frsize as u64))
}

pub fn tamanho_pasta(pasta: &Path) -> u64 {
    let mut total = 0u64;
    let mut pilha = vec![pasta.to_path_buf()];
    while let Some(atual) = pilha.pop() {
        let Ok(itens) = std::fs::read_dir(&atual) else {
            continue;
        };
        for item in itens.flatten() {
            let Ok(meta) = item.path().symlink_metadata() else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                pilha.push(item.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

#[derive(Clone, Copy)]
pub struct Tarefas {
    pub area: bool,
    pub menu: bool,
}

pub fn argumentos_silenciosos(
    _tipo: &str,
    _instalador: &Path,
    _pasta: Option<&Path>,
    _log: &Path,
    _tarefas: Option<Tarefas>,
) -> Option<(PathBuf, String)> {
    None
}

pub struct Atalho {
    pub caminho: PathBuf,
    pub area: bool,
    pub de_todos: bool,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn escapar_desktop(valor: &str) -> String {
    valor
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}

pub fn nome_de_arquivo(nome: &str) -> String {
    let nome: String = nome
        .chars()
        .map(|c| if "/\0\n\r".contains(c) { ' ' } else { c })
        .collect();
    let nome = nome.trim();
    if nome.is_empty() {
        "Aplicativo".into()
    } else {
        nome.into()
    }
}

fn criar_atalho(caminho: &Path, alvo: &Path, pasta: &Path, descricao: &str) -> Result<(), String> {
    if let Some(pai) = caminho.parent() {
        std::fs::create_dir_all(pai).map_err(|e| e.to_string())?;
    }
    let conteudo = format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment={}\nExec=\"{}\"\nPath={}\nTerminal=false\nCategories=Utility;\n",
        escapar_desktop(descricao),
        escapar_desktop(descricao),
        escapar_desktop(&alvo.display().to_string()),
        escapar_desktop(&pasta.display().to_string()),
    );
    std::fs::write(caminho, conteudo).map_err(|e| e.to_string())?;
    let mut permissoes = std::fs::metadata(caminho)
        .map_err(|e| e.to_string())?
        .permissions();
    permissoes.set_mode(0o755);
    std::fs::set_permissions(caminho, permissoes).map_err(|e| e.to_string())
}

fn alvo_do_atalho(caminho: &Path) -> Option<PathBuf> {
    let texto = std::fs::read_to_string(caminho).ok()?;
    let exec = texto.lines().find_map(|l| l.strip_prefix("Exec="))?.trim();
    Some(PathBuf::from(exec.trim_matches('"')))
}

pub fn atalhos_para(pasta: &Path) -> Vec<Atalho> {
    let Some(home) = home() else { return vec![] };
    let mut encontrados = Vec::new();
    for (dir, area) in [
        (home.join("Desktop"), true),
        (home.join(".local/share/applications"), false),
    ] {
        let Ok(itens) = std::fs::read_dir(dir) else {
            continue;
        };
        for item in itens.flatten() {
            let caminho = item.path();
            if caminho.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if alvo_do_atalho(&caminho).is_some_and(|alvo| {
                alvo.canonicalize()
                    .ok()
                    .is_some_and(|a| a.starts_with(pasta))
            }) {
                encontrados.push(Atalho {
                    caminho,
                    area,
                    de_todos: false,
                });
            }
        }
    }
    encontrados
}

pub fn ajustar_atalhos(
    nome: &str,
    exe: &Path,
    pasta: &Path,
    area: bool,
    menu: bool,
) -> Vec<PathBuf> {
    let Some(home) = home() else { return vec![] };
    let arquivo = format!(
        "{}.desktop",
        nome.to_lowercase()
            .replace(|c: char| !c.is_ascii_alphanumeric(), "-")
    );
    let destinos = [
        (area, home.join("Desktop").join(&arquivo)),
        (menu, home.join(".local/share/applications").join(&arquivo)),
    ];
    let mut criados = Vec::new();
    for (criar, caminho) in destinos {
        if criar {
            if criar_atalho(&caminho, exe, pasta, nome).is_ok() {
                criados.push(caminho);
            }
        } else {
            let _ = std::fs::remove_file(caminho);
        }
    }
    criados
}

pub fn apagar_atalho(caminho: &Path) -> bool {
    std::fs::remove_file(caminho).is_ok() || !caminho.exists()
}

fn protegidas() -> Vec<PathBuf> {
    let mut p = vec![
        PathBuf::from("/"),
        PathBuf::from("/usr"),
        PathBuf::from("/etc"),
        PathBuf::from("/var"),
        PathBuf::from("/opt"),
    ];
    if let Some(h) = home() {
        p.extend([
            h.clone(),
            h.join("Desktop"),
            h.join("Documents"),
            h.join("Downloads"),
            h.join(".config"),
            h.join(".local/share"),
        ]);
    }
    p
}

pub fn pasta_apagavel(p: &Path) -> Result<(), String> {
    let meta = p
        .symlink_metadata()
        .map_err(|_| "a pasta não existe mais".to_string())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("não é uma pasta local comum".into());
    }
    let normal = p.canonicalize().map_err(|e| e.to_string())?;
    if normal.components().count() < 5 {
        return Err("fica perto demais da raiz do sistema".into());
    }
    if protegidas().into_iter().any(|x| {
        x.canonicalize()
            .ok()
            .is_some_and(|x| x == normal || x.starts_with(&normal))
    }) {
        return Err("é uma pasta do sistema ou tem uma dentro".into());
    }
    Ok(())
}

pub fn apagar_arvore(p: &Path) -> usize {
    if pasta_apagavel(p).is_err() {
        return 1;
    }
    std::fs::remove_dir_all(p).map_or(1, |_| 0)
}

pub enum Limpeza {
    Pasta(PathBuf),
}

pub fn limpeza(entrada: &str) -> Option<Limpeza> {
    if entrada.contains("..") || entrada.contains('\0') {
        return None;
    }
    let p = PathBuf::from(expandir(entrada)?);
    pasta_apagavel(&p).ok()?;
    Some(Limpeza::Pasta(p))
}

impl Limpeza {
    pub fn existe(&self) -> bool {
        match self {
            Self::Pasta(p) => p.exists(),
        }
    }

    pub fn descricao(&self) -> String {
        match self {
            Self::Pasta(p) => p.display().to_string(),
        }
    }

    pub fn aplicar(&self) -> Result<(), String> {
        match self {
            Self::Pasta(p) if !p.exists() => Ok(()),
            Self::Pasta(p) => pasta_apagavel(p).and_then(|_| match apagar_arvore(p) {
                0 => Ok(()),
                n => Err(format!("{n} item(ns) não removidos")),
            }),
        }
    }
}
