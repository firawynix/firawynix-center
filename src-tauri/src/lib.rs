//! Firawynix Center — launcher dos jogos e projetos Firawynix.
//!
//! Regra de segurança que atravessa o arquivo inteiro: a UI só manda o SLUG (e
//! escolhas de sim/não). URL de download, executável, chave de registro, pasta a
//! instalar ou apagar — tudo sai do catálogo que o Rust baixou por HTTPS ou de
//! uma janela nativa aberta aqui, nunca de um texto vindo da tela.

mod catalogo;
mod download;
mod manifesto;
#[cfg(windows)]
mod sistema;
#[cfg(not(windows))]
#[path = "sistema_linux.rs"]
mod sistema;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_updater::UpdaterExt;

use catalogo::{Catalogo, Jogo, Resposta};
use download::{emitir, ErroDownload};
use manifesto::Modo;
use sistema::{ErroExec, Tarefas};

/// A central. `FIRAWYNIX_CENTER_API` no build aponta para outro ambiente.
pub const API_BASE: &str = match option_env!("FIRAWYNIX_CENTER_API") {
    Some(v) => v,
    None => "https://jogos.firawynix.com.br",
};

/// A edição enviada à Microsoft Store não instala, atualiza nem oferece
/// executáveis. Ela abre projetos web e aplicativos que já estejam instalados.
const STORE_BUILD: bool = cfg!(feature = "store");

/// Erro que a UI sabe mostrar: `codigo` decide o que oferecer (ex.: "desafio"
/// oferece baixar pelo navegador, "atualizacao" oferece jogar mesmo assim).
#[derive(Debug, Clone, Serialize)]
pub struct Falha {
    pub codigo: &'static str,
    pub mensagem: String,
}

impl Falha {
    fn new(codigo: &'static str, mensagem: impl Into<String>) -> Self {
        Self { codigo, mensagem: mensagem.into() }
    }
}

fn interno(e: impl std::fmt::Display) -> Falha {
    Falha::new("interno", e.to_string())
}

impl From<ErroDownload> for Falha {
    fn from(e: ErroDownload) -> Self {
        match e {
            ErroDownload::Desafio => Falha::new(
                "desafio",
                "O servidor pediu verificação de navegador e o launcher não consegue passar por ela.",
            ),
            ErroDownload::Status(s) => Falha::new("rede", format!("O servidor respondeu {s}.")),
            ErroDownload::Rede(e) => Falha::new("rede", format!("Falha de conexão: {e}")),
            ErroDownload::Disco(e) => Falha::new("disco", format!("Não consegui gravar no disco: {e}")),
            ErroDownload::Integridade(e) => Falha::new("integridade", e),
            ErroDownload::Cancelado => Falha::new("cancelado", "Download pausado. Clique de novo para continuar."),
        }
    }
}

pub struct Estado {
    pub http: reqwest::Client,
    pub catalogo: Mutex<Option<Catalogo>>,
    /// rede | cache | embutido — só catálogo fresco autoriza atualizar jogo
    pub origem: Mutex<&'static str>,
    pub ocupados: Mutex<HashSet<String>>,
    pub cancelados: Mutex<HashSet<String>>,
    /// pasta escolhida na janela de instalar: fica aqui, a UI não manda caminho
    pub pastas_escolhidas: Mutex<HashMap<String, PathBuf>>,
    /// versão nova do launcher encontrada pelo updater, esperando a hora de instalar
    pub atualizacao: Mutex<Option<tauri_plugin_updater::Update>>,
}

/// Marca o item como ocupado enquanto vive; solta sozinho em qualquer saída.
struct Ocupado<'a> {
    estado: &'a Estado,
    slug: String,
}

impl<'a> Ocupado<'a> {
    fn pegar(estado: &'a Estado, slug: &str) -> Result<Self, Falha> {
        let mut o = estado.ocupados.lock().map_err(|_| Falha::new("interno", "estado travado"))?;
        if !o.insert(slug.to_string()) {
            return Err(Falha::new("ocupado", "Já tem uma operação em andamento nesse item."));
        }
        if let Ok(mut c) = estado.cancelados.lock() {
            c.remove(slug);
        }
        Ok(Self { estado, slug: slug.to_string() })
    }
}

impl Drop for Ocupado<'_> {
    fn drop(&mut self) {
        if let Ok(mut o) = self.estado.ocupados.lock() {
            o.remove(&self.slug);
        }
    }
}

fn jogo_do_catalogo(estado: &Estado, slug: &str) -> Result<Jogo, Falha> {
    estado
        .catalogo
        .lock()
        .ok()
        .and_then(|c| c.as_ref().and_then(|c| c.jogo(slug).cloned()))
        .ok_or_else(|| Falha::new("invalido", "Item desconhecido. Recarregue a lista."))
}

fn so_https(url: &str) -> Result<url::Url, Falha> {
    let u = url::Url::parse(url).map_err(|_| Falha::new("invalido", "Endereço inválido no catálogo."))?;
    if u.scheme() != "https" {
        return Err(Falha::new("invalido", "O catálogo só pode apontar para endereços https."));
    }
    Ok(u)
}

/// Slug vira nome de arquivo: só o que o painel aceita.
fn slug_valido(slug: &str) -> bool {
    !slug.is_empty() && slug.len() <= 60 && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn pasta_dados(app: &AppHandle) -> Result<PathBuf, Falha> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| Falha::new("disco", format!("Pasta de dados indisponível: {e}")))
}

/* ---------------- o que o launcher lembra de cada jogo ---------------- */

/// slug -> .exe que a pessoa apontou ("Já tenho instalado")
const LOCAIS: &str = "locais.json";
/// slug -> pasta onde o launcher instalou
const PASTAS: &str = "pastas.json";
/// slug -> atalhos que o launcher criou (saem na desinstalação)
const ATALHOS: &str = "atalhos.json";
/// slug -> versão do catálogo já aplicada (evita reinstalar em loop se o
/// instalador gravar outro número no registro)
const VERSOES: &str = "versoes.json";

type Mapa = HashMap<String, String>;
type MapaLista = HashMap<String, Vec<String>>;

fn ler_mapa<T: DeserializeOwned + Default>(app: &AppHandle, nome: &str) -> T {
    pasta_dados(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p.join(nome)).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn mexer_mapa<T, F>(app: &AppHandle, nome: &str, mexer: F) -> Result<(), Falha>
where
    T: DeserializeOwned + Default + Serialize,
    F: FnOnce(&mut T),
{
    let mut valor: T = ler_mapa(app, nome);
    mexer(&mut valor);
    let pasta = pasta_dados(app)?;
    std::fs::create_dir_all(&pasta).map_err(|e| Falha::new("disco", e.to_string()))?;
    let texto = serde_json::to_string_pretty(&valor).map_err(interno)?;
    std::fs::write(pasta.join(nome), texto).map_err(|e| Falha::new("disco", e.to_string()))
}

fn lembrar_pasta(app: &AppHandle, slug: &str, pasta: &Path) {
    let _ = mexer_mapa::<Mapa, _>(app, PASTAS, |m| {
        m.insert(slug.to_string(), pasta.display().to_string());
    });
}

fn lembrar_versao(app: &AppHandle, jogo: &Jogo) {
    if let Some(v) = jogo.versao.clone() {
        let _ = mexer_mapa::<Mapa, _>(app, VERSOES, |m| {
            m.insert(jogo.slug.clone(), v);
        });
    }
}

struct Dados {
    locais: Mapa,
    pastas: Mapa,
}

fn dados(app: &AppHandle) -> Dados {
    Dados { locais: ler_mapa(app, LOCAIS), pastas: ler_mapa(app, PASTAS) }
}

/* ---------------- detecção ---------------- */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EstadoJogo {
    slug: String,
    instalado: bool,
    exe: Option<String>,
    pasta: Option<String>,
    versao_instalada: Option<String>,
    desinstalavel: bool,
    /// registro | pasta | caminho | localizado
    origem: Option<&'static str>,
    /// instalação por manifesto que não terminou
    incompleto: bool,
}

struct Deteccao {
    exe: PathBuf,
    pasta: PathBuf,
    versao: Option<String>,
    desinstalar: Option<String>,
    da_maquina: bool,
    origem: &'static str,
}

fn exe_na_pasta(jogo: &Jogo, pasta: &Path) -> Option<PathBuf> {
    let rel = jogo.win_exe.as_deref().filter(|r| sistema::relativo_seguro(r))?;
    let exe = pasta.join(rel);
    (sistema::e_exe(&exe) && sistema::dentro(pasta, &exe)).then_some(exe)
}

fn detectar(jogo: &Jogo, d: &Dados) -> Option<Deteccao> {
    // 1) a pessoa apontou o executável
    if let Some(p) = d.locais.get(&jogo.slug).map(PathBuf::from) {
        if sistema::e_exe(&p) {
            let pasta = p.parent().map(Path::to_path_buf).unwrap_or_default();
            return Some(Deteccao { exe: p, pasta, versao: None, desinstalar: None, da_maquina: false, origem: "localizado" });
        }
    }
    // 2) o registro do instalador (Inno, NSIS, MSI, setup próprio)
    if let Some(reg) = jogo.win_chave.as_deref().and_then(sistema::ler_registro) {
        let exe = match (&reg.pasta, jogo.win_exe.as_deref()) {
            (Some(pasta), Some(rel)) if sistema::relativo_seguro(rel) => {
                let e = pasta.join(rel);
                sistema::dentro(pasta, &e).then_some(e)
            }
            _ => reg.icone.clone().filter(|i| sistema::e_exe(i)),
        };
        if let (Some(exe), Some(pasta)) = (exe, reg.pasta.clone()) {
            if sistema::e_exe(&exe) {
                return Some(Deteccao {
                    exe,
                    pasta,
                    versao: reg.versao,
                    desinstalar: reg.desinstalar,
                    da_maquina: reg.da_maquina,
                    origem: "registro",
                });
            }
        }
    }
    // 3) a pasta onde o launcher instalou (escolhida na janela de instalar)
    if let Some(pasta) = d.pastas.get(&jogo.slug).map(PathBuf::from) {
        if let Some(exe) = exe_na_pasta(jogo, &pasta) {
            return Some(Deteccao { exe, pasta, versao: None, desinstalar: None, da_maquina: false, origem: "pasta" });
        }
    }
    // 4) caminhos candidatos do catálogo
    for candidato in &jogo.win_caminhos {
        let Some(expandido) = sistema::expandir(candidato) else { continue };
        if expandido.contains("..") {
            continue;
        }
        let p = PathBuf::from(expandido);
        if sistema::e_exe(&p) {
            let pasta = p.parent().map(Path::to_path_buf).unwrap_or_default();
            return Some(Deteccao { exe: p, pasta, versao: None, desinstalar: None, da_maquina: false, origem: "caminho" });
        }
    }
    None
}

/// Pasta padrão: a do 1º caminho candidato do catálogo.
fn pasta_do_jogo(jogo: &Jogo) -> Option<PathBuf> {
    jogo.win_caminhos
        .iter()
        .find_map(|c| sistema::expandir(c))
        .filter(|p| !p.contains(".."))
        .and_then(|p| PathBuf::from(p).parent().map(Path::to_path_buf))
}

/// Onde o jogo está (ou estaria): a lembrada, senão a padrão.
fn pasta_lembrada(jogo: &Jogo, d: &Dados) -> Option<PathBuf> {
    d.pastas.get(&jogo.slug).map(PathBuf::from).or_else(|| pasta_do_jogo(jogo))
}

/// Para onde a próxima instalação vai: a escolhida agora, a lembrada ou a padrão.
fn pasta_alvo(estado: &Estado, jogo: &Jogo, d: &Dados) -> Option<PathBuf> {
    estado
        .pastas_escolhidas
        .lock()
        .ok()
        .and_then(|m| m.get(&jogo.slug).cloned())
        .or_else(|| pasta_lembrada(jogo, d))
}

fn pela_metade(jogo: &Jogo, pasta: &Path) -> bool {
    jogo.por_manifesto() && pasta.join(manifesto::EM_ANDAMENTO).is_file()
}

fn estado_de(jogo: &Jogo, d: &Dados) -> EstadoJogo {
    match detectar(jogo, d) {
        Some(det) => {
            // por manifesto, o .exe pode chegar antes do resto: sem a marca do fim, está pela metade
            let incompleto = jogo.por_manifesto()
                && det.origem != "localizado"
                && (pela_metade(jogo, &det.pasta) || !det.pasta.join(manifesto::MARCA).is_file());
            EstadoJogo {
                slug: jogo.slug.clone(),
                instalado: !incompleto,
                exe: Some(det.exe.to_string_lossy().into_owned()),
                pasta: Some(det.pasta.to_string_lossy().into_owned()),
                versao_instalada: det.versao,
                desinstalavel: det.desinstalar.is_some()
                    || (det.origem != "localizado" && (jogo.por_manifesto() || det.origem == "pasta")),
                origem: Some(det.origem),
                incompleto,
            }
        }
        None => {
            let metade = pasta_lembrada(jogo, d).filter(|p| pela_metade(jogo, p));
            EstadoJogo {
                slug: jogo.slug.clone(),
                instalado: false,
                exe: None,
                pasta: metade.as_ref().map(|p| p.to_string_lossy().into_owned()),
                versao_instalada: None,
                desinstalavel: metade.is_some(),
                origem: None,
                incompleto: metade.is_some(),
            }
        }
    }
}

/* ---------------- eventos para a central (contadores) ---------------- */

fn avisar_central(estado: &Estado, slug: &str, tipo: &'static str) {
    let http = estado.http.clone();
    let url = format!("{API_BASE}/api/games/{slug}/evento");
    tauri::async_runtime::spawn(async move {
        let _ = http
            .post(url)
            .json(&serde_json::json!({ "tipo": tipo }))
            .timeout(Duration::from_secs(10))
            .send()
            .await;
    });
}

/* ---------------- catálogo e estados ---------------- */

#[tauri::command]
async fn catalogo(app: AppHandle, estado: State<'_, Estado>) -> Result<Resposta, Falha> {
    let cache = pasta_dados(&app)?.join("catalogo.json");
    let (mut cat, origem, aviso) = match catalogo::buscar(&estado.http).await {
        Ok(c) => {
            catalogo::gravar_cache(&cache, &c);
            (c, "rede", None)
        }
        Err(e) => {
            let motivo = e.texto();
            match catalogo::ler_cache(&cache) {
                Some(c) => (c, "cache", Some(format!("Sem conexão com a central ({motivo}). Mostrando a última lista salva."))),
                None => (
                    catalogo::embutido(),
                    "embutido",
                    Some(format!("Sem conexão com a central ({motivo}). Mostrando a lista que veio com o launcher.")),
                ),
            }
        }
    };

    if STORE_BUILD {
        let d = dados(&app);
        let manter = |j: &Jogo| j.e_web() || detectar(j, &d).is_some();
        cat.jogos.retain(manter);
        cat.projetos.retain(manter);
        for j in cat.jogos.iter_mut().chain(cat.projetos.iter_mut()) {
            j.download_url = None;
            j.download_tamanho = None;
            j.download_sha256 = None;
            j.manifesto_url = None;
            j.extras.clear();
        }
        cat.launcher = None;
    }

    let versao_app = app.package_info().version.to_string();
    let atualizacao = cat
        .launcher
        .as_ref()
        .map_or(false, |l| catalogo::versao_maior(&l.versao, &versao_app));
    let resposta = Resposta {
        jogos: cat.jogos.clone(),
        projetos: cat.projetos.clone(),
        launcher: cat.launcher.clone(),
        origem,
        aviso,
        versao_app,
        atualizacao,
        base: API_BASE.to_string(),
        plataforma: if cfg!(target_os = "linux") { "linux" } else { "windows" },
    };
    *estado.catalogo.lock().map_err(|_| Falha::new("interno", "estado travado"))? = Some(cat);
    if let Ok(mut o) = estado.origem.lock() {
        *o = origem;
    }
    Ok(resposta)
}

#[tauri::command]
fn estados(app: AppHandle, estado: State<'_, Estado>) -> Vec<EstadoJogo> {
    let d = dados(&app);
    estado
        .catalogo
        .lock()
        .ok()
        .and_then(|c| c.clone())
        .map(|c| c.itens().filter(|j| !j.e_web()).map(|j| estado_de(j, &d)).collect())
        .unwrap_or_default()
}

/* ---------------- instalar ---------------- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanoInstalacao {
    pasta: Option<String>,
    /// instalador silencioso ou manifesto aceitam outra pasta; o manual decide na janela dele
    pode_escolher: bool,
    livre: Option<u64>,
    necessario: Option<u64>,
    silencioso: bool,
}

fn plano_instalacao(estado: &Estado, jogo: &Jogo, d: &Dados) -> PlanoInstalacao {
    let pasta = pasta_alvo(estado, jogo, d);
    let livre = pasta
        .as_deref()
        .and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf))
        .and_then(|p| sistema::espaco_livre(&p));
    PlanoInstalacao {
        pasta: pasta.map(|p| p.display().to_string()),
        pode_escolher: jogo.por_manifesto() || matches!(jogo.win_instalador.as_str(), "inno" | "nsis" | "appimage"),
        livre,
        necessario: jogo.instalado_tamanho.or(jogo.download_tamanho),
        silencioso: jogo.silencioso() || jogo.por_manifesto(),
    }
}

/// A janela de instalar abriu: volta para a pasta lembrada/padrão.
#[tauri::command]
fn preparar_instalacao(app: AppHandle, estado: State<'_, Estado>, slug: String) -> Result<PlanoInstalacao, Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    if let Ok(mut m) = estado.pastas_escolhidas.lock() {
        m.remove(&slug);
    }
    Ok(plano_instalacao(&estado, &jogo, &dados(&app)))
}

/// "Alterar pasta": a janela é do Windows e o caminho fica no Rust. A pasta do
/// jogo é criada DENTRO da escolhida (`D:\Jogos` -> `D:\Jogos\Aion H4K`), então a
/// desinstalação completa só apaga o que é do jogo.
#[tauri::command]
async fn escolher_pasta(app: AppHandle, estado: State<'_, Estado>, slug: String) -> Result<Option<PlanoInstalacao>, Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    let Some(escolhida) = app
        .dialog()
        .file()
        .set_title(format!("Onde instalar o {}?", jogo.nome))
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let base = escolhida
        .into_path()
        .map_err(|e| Falha::new("invalido", format!("Pasta inválida: {e}")))?;
    let nome = pasta_do_jogo(&jogo)
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| sistema::nome_de_arquivo(&jogo.nome));
    let pasta = if base.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&nome)) {
        base
    } else {
        base.join(&nome)
    };
    if !pasta.is_absolute() || pasta.to_string_lossy().starts_with(r"\\") {
        return Err(Falha::new("invalido", "Escolha uma pasta num disco deste computador."));
    }
    if let Ok(mut m) = estado.pastas_escolhidas.lock() {
        m.insert(slug, pasta);
    }
    Ok(Some(plano_instalacao(&estado, &jogo, &dados(&app))))
}

/// Baixa o instalador do catálogo e roda: silencioso (a barra mede a pasta
/// crescer) ou com a janela dele. Serve para instalar e para atualizar.
async fn rodar_instalador(
    app: &AppHandle,
    estado: &Estado,
    jogo: &Jogo,
    pasta_jogo: Option<PathBuf>,
    tarefas: Option<Tarefas>,
) -> Result<(), Falha> {
    let url = jogo
        .download_url
        .clone()
        .ok_or_else(|| Falha::new("invalido", "O catálogo não tem o link do instalador."))?;
    so_https(&url)?;
    let extensao = if jogo.win_instalador == "appimage" { "AppImage" } else { "exe" };
    let destino = pasta_dados(app)?.join("downloads").join(format!("{}-setup.{extensao}", jogo.slug));
    download::baixar(app, estado, &jogo.slug, &url, &destino, jogo.download_tamanho, jogo.download_sha256.as_deref())
        .await
        .map_err(Falha::from)?;

    #[cfg(target_os = "linux")]
    if jogo.win_instalador == "appimage" {
        use std::os::unix::fs::PermissionsExt;
        let pasta = pasta_jogo.ok_or_else(|| Falha::new("invalido", "O catálogo não informa a pasta do aplicativo."))?;
        let nome = jogo
            .win_exe
            .as_deref()
            .filter(|n| sistema::relativo_seguro(n))
            .ok_or_else(|| Falha::new("invalido", "O catálogo não informa o executável Linux."))?;
        std::fs::create_dir_all(&pasta).map_err(|e| Falha::new("disco", e.to_string()))?;
        let final_app = pasta.join(nome);
        let temporario = pasta.join(format!(".{nome}.novo"));
        let _ = std::fs::remove_file(&temporario);
        std::fs::copy(&destino, &temporario).map_err(|e| Falha::new("disco", e.to_string()))?;
        let mut permissoes = std::fs::metadata(&temporario).map_err(interno)?.permissions();
        permissoes.set_mode(permissoes.mode() | 0o755);
        std::fs::set_permissions(&temporario, permissoes).map_err(interno)?;
        std::fs::rename(&temporario, &final_app).map_err(|e| Falha::new("disco", e.to_string()))?;
        let _ = std::fs::remove_file(&destino);
        emitir(app, &jogo.slug, "instalando", 1, Some(1), 0);
        return Ok(());
    }

    let log = destino.with_extension("log");
    let silencioso = if jogo.silencioso() {
        sistema::argumentos_silenciosos(&jogo.win_instalador, &destino, pasta_jogo.as_deref(), &log, tarefas)
    } else {
        None
    };

    let codigo = match silencioso {
        Some((alvo, args)) => {
            // sem janela: a barra anda medindo a pasta crescer
            let terminou = Arc::new(AtomicBool::new(false));
            let vigia = pasta_jogo.clone().map(|pasta| {
                let (app2, slug2, fim) = (app.clone(), jogo.slug.clone(), terminou.clone());
                let total = jogo.instalado_tamanho;
                std::thread::spawn(move || {
                    let base = sistema::tamanho_pasta(&pasta);
                    // pasta já cheia (reinstalação): crescer não diz nada, barra sem porcentagem
                    let total_util = total.filter(|t| base < t / 2).map(|t| t - base);
                    while !fim.load(Ordering::Relaxed) {
                        let gravado = sistema::tamanho_pasta(&pasta).saturating_sub(base);
                        download::emitir_instalacao(&app2, &slug2, gravado, total_util);
                        std::thread::sleep(Duration::from_millis(900));
                    }
                })
            });
            if vigia.is_none() {
                download::emitir_instalacao(app, &jogo.slug, 0, None);
            }
            let resultado = tauri::async_runtime::spawn_blocking(move || sistema::executar_oculto(&alvo, Some(&args))).await;
            terminou.store(true, Ordering::Relaxed);
            if let Some(v) = vigia {
                let _ = tauri::async_runtime::spawn_blocking(move || v.join()).await;
            }
            resultado
        }
        None => {
            // instalador manual: a janela dele abre e a pessoa conclui
            emitir(app, &jogo.slug, "instalando", 0, None, 0);
            let alvo = destino.clone();
            tauri::async_runtime::spawn_blocking(move || sistema::executar(&alvo, None, None, true)).await
        }
    }
    .map_err(interno)?
    .map_err(|e| match e {
        ErroExec::Cancelado => Falha::new("uac", "A instalação precisa de permissão de administrador e foi recusada."),
        ErroExec::Outro(m) => Falha::new("instalador", format!("Não consegui abrir o instalador: {m}")),
    })?;

    match codigo {
        Some(0) | None => {
            // centenas de MB a menos no disco: o instalador não serve mais para nada
            let _ = tokio::fs::remove_file(&destino).await;
            let _ = tokio::fs::remove_file(&log).await;
            Ok(())
        }
        // Inno: 2 = cancelado pela pessoa antes de começar, 5 = cancelado no meio
        Some(2) | Some(5) => Err(Falha::new("cancelado", "Instalação cancelada no instalador. O arquivo baixado ficou guardado.")),
        Some(c) if log.is_file() => Err(Falha::new(
            "instalador",
            format!("O instalador terminou com o código {c}. Detalhes em {}", log.display()),
        )),
        Some(c) => Err(Falha::new("instalador", format!("O instalador terminou com o código {c}."))),
    }
}

/// Cria/tira atalhos conforme a escolha e anota os que o launcher criou.
async fn aplicar_atalhos(app: &AppHandle, jogo: &Jogo, exe: &Path, pasta: &Path, area: bool, menu: bool) -> Result<(), Falha> {
    let (nome, exe, pasta) = (sistema::nome_de_arquivo(&jogo.nome), exe.to_path_buf(), pasta.to_path_buf());
    let criados = tauri::async_runtime::spawn_blocking(move || sistema::ajustar_atalhos(&nome, &exe, &pasta, area, menu))
        .await
        .map_err(interno)?;
    if !criados.is_empty() {
        mexer_mapa::<MapaLista, _>(app, ATALHOS, |m| {
            let lista = m.entry(jogo.slug.clone()).or_default();
            for c in criados {
                let c = c.display().to_string();
                if !lista.contains(&c) {
                    lista.push(c);
                }
            }
        })?;
    }
    Ok(())
}

#[tauri::command]
async fn instalar(app: AppHandle, estado: State<'_, Estado>, slug: String, area: bool, menu: bool) -> Result<EstadoJogo, Falha> {
    if STORE_BUILD {
        return Err(Falha::new("store", "A edição da Microsoft Store abre somente projetos web e aplicativos já instalados."));
    }
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    if jogo.e_web() || !jogo.disponivel || !slug_valido(&jogo.slug) {
        return Err(Falha::new("invalido", "Esse item não tem instalador."));
    }
    let pasta = pasta_alvo(&estado, &jogo, &dados(&app));
    let _ocupado = Ocupado::pegar(&estado, &slug)?;
    let aceita_pasta = matches!(jogo.win_instalador.as_str(), "inno" | "nsis" | "appimage");

    if jogo.por_manifesto() {
        // sem instalador nenhum: o launcher baixa o jogo arquivo por arquivo
        let url = jogo
            .manifesto_url
            .clone()
            .ok_or_else(|| Falha::new("invalido", "O catálogo não tem o link do manifesto."))?;
        so_https(&url)?;
        let pasta = pasta.ok_or_else(|| Falha::new("invalido", "O catálogo não diz em que pasta instalar (caminho candidato)."))?;
        // lembrada ANTES: interrompido no meio, o launcher sabe onde continuar
        lembrar_pasta(&app, &slug, &pasta);
        manifesto::sincronizar(&app, &estado, &jogo.slug, &url, &pasta, jogo.win_exe.as_deref(), Modo::Instalar).await?;
    } else {
        let tarefas = jogo.silencioso().then_some(Tarefas { area, menu });
        rodar_instalador(&app, &estado, &jogo, pasta.clone().filter(|_| aceita_pasta), tarefas).await?;
        if let Some(p) = pasta.filter(|_| aceita_pasta) {
            lembrar_pasta(&app, &slug, &p);
        }
    }
    lembrar_versao(&app, &jogo);
    if let Ok(mut m) = estado.pastas_escolhidas.lock() {
        m.remove(&slug);
    }

    let d = dados(&app);
    // o instalador manual (setup próprio) pergunta dos atalhos na janela dele
    if jogo.silencioso() || jogo.por_manifesto() {
        if let Some(det) = detectar(&jogo, &d) {
            let _ = aplicar_atalhos(&app, &jogo, &det.exe, &det.pasta, area, menu).await;
        }
    }
    emitir(&app, &slug, "concluido", 0, None, 0);
    avisar_central(&estado, &slug, "instalar");
    Ok(estado_de(&jogo, &d))
}

#[tauri::command]
fn cancelar(estado: State<'_, Estado>, slug: String) {
    if let Ok(mut c) = estado.cancelados.lock() {
        c.insert(slug);
    }
}

/// Atalhos para um jogo que já está instalado (menu "Criar atalhos").
#[tauri::command]
async fn criar_atalhos(app: AppHandle, estado: State<'_, Estado>, slug: String, area: bool, menu: bool) -> Result<(), Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    let det = detectar(&jogo, &dados(&app)).ok_or_else(|| Falha::new("ausente", "O jogo não está instalado."))?;
    aplicar_atalhos(&app, &jogo, &det.exe, &det.pasta, area, menu).await
}

/* ---------------- jogar (com atualização antes) ---------------- */

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct ResultadoJogar {
    atualizado: bool,
    aviso: Option<String>,
}

/// Antes de abrir: manifesto -> baixa o que o servidor mudou; instalador ->
/// versão do catálogo maior que a do registro roda o instalador novo por cima.
/// `Ok(true)` = atualizou alguma coisa. "sem_rede" = não deu para conferir.
async fn atualizar_jogo(app: &AppHandle, estado: &Estado, jogo: &Jogo) -> Result<bool, Falha> {
    emitir(app, &jogo.slug, "verificando", 0, None, 0);
    let fresco = estado.origem.lock().map(|o| *o == "rede").unwrap_or(false);
    if !fresco {
        return Err(Falha::new("sem_rede", "Sem conexão com a central."));
    }
    let d = dados(app);
    let Some(det) = detectar(jogo, &d) else { return Ok(false) };
    if det.origem == "localizado" {
        // instalação de fora do launcher: quem atualiza é o launcher do próprio jogo
        return Ok(false);
    }
    match jogo.win_instalador.as_str() {
        "manifesto" => {
            let Some(url) = jogo.manifesto_url.as_deref() else { return Ok(false) };
            so_https(url)?;
            let r = manifesto::sincronizar(app, estado, &jogo.slug, url, &det.pasta, jogo.win_exe.as_deref(), Modo::Atualizar).await?;
            Ok(r.baixados + r.apagados > 0)
        }
        "appimage" => {
            let (Some(publicada), aplicadas) = (jogo.versao.as_deref(), ler_mapa::<Mapa>(app, VERSOES)) else {
                return Ok(false);
            };
            if aplicadas.get(&jogo.slug).map(String::as_str) == Some(publicada) {
                return Ok(false);
            }
            rodar_instalador(app, estado, jogo, Some(det.pasta.clone()), None).await?;
            lembrar_versao(app, jogo);
            Ok(true)
        }
        "inno" | "nsis" | "msi" => {
            let (Some(instalada), Some(publicada)) = (det.versao.as_deref(), jogo.versao.as_deref()) else {
                return Ok(false);
            };
            let aplicadas: Mapa = ler_mapa(app, VERSOES);
            if !catalogo::versao_maior(publicada, instalada) || aplicadas.get(&jogo.slug).map(String::as_str) == Some(publicada) {
                return Ok(false);
            }
            // por cima da instalação atual; o Inno repete as escolhas de atalho da 1ª vez
            rodar_instalador(app, estado, jogo, Some(det.pasta.clone()), None).await?;
            lembrar_versao(app, jogo);
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[tauri::command]
async fn jogar(app: AppHandle, estado: State<'_, Estado>, slug: String, pular: bool) -> Result<ResultadoJogar, Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    if jogo.e_web() {
        return Err(Falha::new("invalido", "Esse abre numa janela do launcher."));
    }
    let _ocupado = Ocupado::pegar(&estado, &slug)?;
    let mut r = ResultadoJogar::default();
    if !pular && !STORE_BUILD {
        match atualizar_jogo(&app, &estado, &jogo).await {
            Ok(mudou) => r.atualizado = mudou,
            Err(f) if f.codigo == "sem_rede" => {
                r.aviso = Some("Sem conexão com o servidor: abrindo sem procurar atualização.".into());
            }
            Err(f) if matches!(f.codigo, "cancelado" | "uac" | "espaco") => return Err(f),
            Err(f) => return Err(Falha::new("atualizacao", format!("A atualização falhou: {}", f.mensagem))),
        }
    }
    let d = detectar(&jogo, &dados(&app))
        .ok_or_else(|| Falha::new("ausente", "Não encontrei o jogo instalado. Instale ou localize a pasta."))?;
    tauri::async_runtime::spawn_blocking(move || sistema::executar(&d.exe, None, Some(&d.pasta), false))
        .await
        .map_err(interno)?
        .map_err(|e| match e {
            ErroExec::Cancelado => Falha::new("uac", "O jogo pede permissão de administrador e ela foi recusada."),
            ErroExec::Outro(m) => Falha::new("abrir", format!("Não consegui abrir: {m}")),
        })?;
    emitir(&app, &slug, "concluido", 0, None, 0);
    avisar_central(&estado, &slug, "jogar");
    Ok(r)
}

/// Jogo de navegador ou projeto web numa janela do próprio launcher. A janela
/// abre a página de fora SEM acesso ao Rust (a capability "default" só vale
/// para a "main").
#[tauri::command]
async fn jogar_web(app: AppHandle, estado: State<'_, Estado>, slug: String) -> Result<(), Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    let url = so_https(jogo.jogar_url.as_deref().unwrap_or_default())?;
    let rotulo = format!("jogo-{}", jogo.slug);
    if let Some(janela) = app.get_webview_window(&rotulo) {
        let _ = janela.unminimize();
        let _ = janela.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, &rotulo, WebviewUrl::External(url))
        .title(format!("{} — Firawynix Center", jogo.nome))
        .inner_size(1280.0, 800.0)
        .min_inner_size(800.0, 560.0)
        .center()
        .build()
        .map_err(|e| Falha::new("abrir", format!("Não consegui abrir a janela: {e}")))?;
    avisar_central(&estado, &slug, "jogar");
    Ok(())
}

/* ---------------- desinstalar ---------------- */

struct AlvoDesinstalar {
    pasta: Option<PathBuf>,
    comando: Option<sistema::ComandoDesinstalar>,
    da_maquina: bool,
    localizado: bool,
    registro: bool,
}

fn alvo_desinstalar(jogo: &Jogo, d: &Dados) -> Option<AlvoDesinstalar> {
    match detectar(jogo, d) {
        Some(det) => Some(AlvoDesinstalar {
            comando: det.desinstalar.as_deref().map(|c| sistema::comando_desinstalar(&jogo.win_instalador, c)),
            pasta: Some(det.pasta),
            da_maquina: det.da_maquina,
            localizado: det.origem == "localizado",
            registro: det.origem == "registro",
        }),
        // download pela metade, sem .exe ainda: dá para descartar
        None => pasta_lembrada(jogo, d).filter(|p| pela_metade(jogo, p)).map(|p| AlvoDesinstalar {
            pasta: Some(p),
            comando: None,
            da_maquina: false,
            localizado: false,
            registro: false,
        }),
    }
}

fn so_localizado() -> Falha {
    Falha::new("invalido", "Esse foi localizado à mão, o launcher não o instalou. Use \"Esquecer este local\".")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanoDesinstalar {
    pasta: Option<String>,
    silencioso: bool,
    precisa_admin: bool,
    /// sem desinstalador (manifesto): desinstalar = apagar a pasta
    apaga_pasta: bool,
    /// o que a remoção completa leva a mais
    completa: Vec<String>,
    atalhos: Vec<String>,
    avisos: Vec<String>,
}

#[tauri::command]
async fn plano_desinstalar(app: AppHandle, estado: State<'_, Estado>, slug: String) -> Result<PlanoDesinstalar, Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    let alvo = alvo_desinstalar(&jogo, &dados(&app)).ok_or_else(|| Falha::new("ausente", "Não encontrei o jogo instalado."))?;
    if alvo.localizado && alvo.comando.is_none() {
        return Err(so_localizado());
    }
    let (pasta, localizado, limpeza) = (alvo.pasta.clone(), alvo.localizado, jogo.win_limpeza.clone());
    let (atalhos, completa, avisos, apagavel) = tauri::async_runtime::spawn_blocking(move || {
        let atalhos: Vec<String> = pasta
            .as_deref()
            .map(sistema::atalhos_para)
            .unwrap_or_default()
            .into_iter()
            .map(|a| a.caminho.display().to_string())
            .collect();
        let (mut completa, mut avisos, mut apagavel) = (Vec::new(), Vec::new(), false);
        if let Some(p) = pasta.as_deref().filter(|p| p.exists()) {
            if localizado {
                avisos.push(format!("A pasta {} fica: foi você que apontou o jogo.", p.display()));
            } else {
                match sistema::pasta_apagavel(p) {
                    Ok(()) => {
                        apagavel = true;
                        completa.push(format!("{} (a pasta inteira)", p.display()));
                    }
                    Err(m) => avisos.push(format!("A pasta {} não será apagada: {m}.", p.display())),
                }
            }
        }
        for e in &limpeza {
            match sistema::limpeza(e) {
                Some(l) if l.existe() => completa.push(l.descricao()),
                Some(_) => {}
                None => avisos.push(format!("Item de limpeza recusado pelo launcher: {e}")),
            }
        }
        completa.push("O que o Firawynix Center guardou sobre ele (instalador baixado, pasta lembrada)".into());
        (atalhos, completa, avisos, apagavel)
    })
    .await
    .map_err(interno)?;
    Ok(PlanoDesinstalar {
        pasta: alvo.pasta.map(|p| p.display().to_string()),
        silencioso: alvo.comando.as_ref().map_or(true, |c| c.oculto),
        precisa_admin: alvo.da_maquina,
        apaga_pasta: alvo.comando.is_none() && apagavel,
        completa,
        atalhos,
        avisos,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultadoDesinstalar {
    estado: EstadoJogo,
    /// o que não saiu (arquivo em uso, atalho de todos os usuários...)
    restou: Vec<String>,
}

#[tauri::command]
async fn desinstalar(app: AppHandle, estado: State<'_, Estado>, slug: String, completo: bool) -> Result<ResultadoDesinstalar, Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    let alvo = alvo_desinstalar(&jogo, &dados(&app)).ok_or_else(|| Falha::new("ausente", "Não encontrei o jogo instalado."))?;
    if alvo.localizado && alvo.comando.is_none() {
        return Err(so_localizado());
    }
    let _ocupado = Ocupado::pegar(&estado, &slug)?;
    emitir(&app, &slug, "desinstalando", 0, None, 0);
    let mut restou = Vec::new();

    // 1) o desinstalador do próprio jogo — sem janela quando dá (Inno, NSIS, MSI)
    if let Some(cmd) = alvo.comando {
        let chave = if alvo.registro { jogo.win_chave.clone() } else { None };
        let codigo = tauri::async_runtime::spawn_blocking(move || {
            let r = if cmd.oculto {
                sistema::executar_oculto(&cmd.exe, cmd.args.as_deref())
            } else {
                sistema::executar(&cmd.exe, cmd.args.as_deref(), None, true)
            };
            // o Inno termina num processo filho: a chave do registro some no fim
            if matches!(r, Ok(Some(0))) {
                if let Some(c) = chave {
                    let limite = Instant::now() + Duration::from_secs(60);
                    while sistema::ler_registro(&c).is_some() && Instant::now() < limite {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
            }
            r
        })
        .await
        .map_err(interno)?
        .map_err(|e| match e {
            ErroExec::Cancelado => Falha::new("uac", "A desinstalação precisa de permissão de administrador e foi recusada."),
            ErroExec::Outro(m) => Falha::new("instalador", format!("Não consegui abrir o desinstalador: {m}")),
        })?;
        if let Some(c) = codigo.filter(|c| *c != 0) {
            return Err(Falha::new("instalador", format!("O desinstalador terminou com o código {c}.")));
        }
    } else if let Some(p) = alvo.pasta.clone() {
        // por manifesto não existe desinstalador: desinstalar É apagar a pasta do jogo
        let r = tauri::async_runtime::spawn_blocking(move || sistema::pasta_apagavel(&p).map(|_| (sistema::apagar_arvore(&p), p)))
            .await
            .map_err(interno)?;
        match r {
            Ok((0, _)) => {}
            Ok((n, p)) => restou.push(format!("{} ({n} item(ns) em uso — feche o jogo e tente de novo)", p.display())),
            Err(m) => return Err(Falha::new("invalido", format!("Não vou apagar essa pasta: {m}."))),
        }
    }

    // 2) atalhos: os que o launcher criou e qualquer um que ainda aponte para a pasta
    let nossos = ler_mapa::<MapaLista>(&app, ATALHOS).remove(&slug).unwrap_or_default();
    let pasta = alvo.pasta.clone();
    let sobra_atalhos = tauri::async_runtime::spawn_blocking(move || {
        for a in &nossos {
            sistema::apagar_atalho(Path::new(a));
        }
        pasta
            .map(|p| sistema::atalhos_para(&p))
            .unwrap_or_default()
            .into_iter()
            .filter(|a| !sistema::apagar_atalho(&a.caminho))
            .map(|a| {
                let dono = if a.de_todos { " (de todos os usuários: precisa de administrador)" } else { "" };
                format!("Atalho {}{dono}", a.caminho.display())
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    restou.extend(sobra_atalhos);

    // 3) completa: o que sobrou da pasta, pastas e registro do jogo, dados do launcher
    if completo {
        let (pasta, localizado, limpeza, chave) = (alvo.pasta.clone(), alvo.localizado, jogo.win_limpeza.clone(), jogo.win_chave.clone());
        let mais = tauri::async_runtime::spawn_blocking(move || {
            let mut restou = Vec::new();
            if let Some(p) = pasta.filter(|p| p.exists() && !localizado) {
                match sistema::pasta_apagavel(&p) {
                    Ok(()) => match sistema::apagar_arvore(&p) {
                        0 => {}
                        n => restou.push(format!("{} ({n} item(ns) em uso)", p.display())),
                    },
                    Err(m) => restou.push(format!("{}: {m}", p.display())),
                }
            }
            for e in &limpeza {
                if let Some(l) = sistema::limpeza(e) {
                    if let Err(m) = l.aplicar() {
                        restou.push(format!("{}: {m}", l.descricao()));
                    }
                }
            }
            if let Some(c) = chave {
                sistema::apagar_chave_desinstalar(&c);
            }
            restou
        })
        .await
        .unwrap_or_default();
        restou.extend(mais);
        if let Ok(p) = pasta_dados(&app) {
            for ext in ["exe", "part", "log"] {
                let _ = std::fs::remove_file(p.join("downloads").join(format!("{slug}-setup.{ext}")));
            }
        }
        let _ = mexer_mapa::<Mapa, _>(&app, LOCAIS, |m| {
            m.remove(&slug);
        });
    }
    let _ = mexer_mapa::<Mapa, _>(&app, PASTAS, |m| {
        m.remove(&slug);
    });
    let _ = mexer_mapa::<Mapa, _>(&app, VERSOES, |m| {
        m.remove(&slug);
    });
    let _ = mexer_mapa::<MapaLista, _>(&app, ATALHOS, |m| {
        m.remove(&slug);
    });
    emitir(&app, &slug, "concluido", 0, None, 0);
    Ok(ResultadoDesinstalar { estado: estado_de(&jogo, &dados(&app)), restou })
}

/* ---------------- já instalado por fora ---------------- */

/// "Já tenho instalado": a pessoa aponta o executável e o launcher lembra.
#[tauri::command]
async fn localizar(app: AppHandle, estado: State<'_, Estado>, slug: String) -> Result<Option<EstadoJogo>, Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    let escolhido = app
        .dialog()
        .file()
        .set_title(format!("Onde está o {}?", jogo.nome))
        .add_filter(
            if cfg!(target_os = "linux") { "Aplicativo" } else { "Executável" },
            if cfg!(target_os = "linux") { &["AppImage", "appimage"][..] } else { &["exe"][..] },
        )
        .blocking_pick_file();
    let Some(escolhido) = escolhido else { return Ok(None) };
    let caminho = escolhido
        .into_path()
        .map_err(|e| Falha::new("invalido", format!("Caminho inválido: {e}")))?;
    if !sistema::e_exe(&caminho) {
        return Err(Falha::new(
            "invalido",
            if cfg!(target_os = "linux") { "Escolha um aplicativo executável." } else { "Escolha o arquivo .exe." },
        ));
    }
    mexer_mapa::<Mapa, _>(&app, LOCAIS, |m| {
        m.insert(jogo.slug.clone(), caminho.to_string_lossy().into_owned());
    })?;
    Ok(Some(estado_de(&jogo, &dados(&app))))
}

#[tauri::command]
fn esquecer(app: AppHandle, estado: State<'_, Estado>, slug: String) -> Result<EstadoJogo, Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    mexer_mapa::<Mapa, _>(&app, LOCAIS, |m| {
        m.remove(&jogo.slug);
    })?;
    Ok(estado_de(&jogo, &dados(&app)))
}

#[tauri::command]
async fn abrir_pasta(app: AppHandle, estado: State<'_, Estado>, slug: String) -> Result<(), Falha> {
    let jogo = jogo_do_catalogo(&estado, &slug)?;
    let d = detectar(&jogo, &dados(&app)).ok_or_else(|| Falha::new("ausente", "Não está instalado."))?;
    if !d.pasta.is_dir() {
        return Err(Falha::new("ausente", "A pasta não existe mais."));
    }
    tauri::async_runtime::spawn_blocking(move || sistema::executar(&d.pasta, None, None, false))
        .await
        .map_err(interno)?
        .map_err(|_| Falha::new("abrir", "Não consegui abrir a pasta."))?;
    Ok(())
}

/// Só abre no navegador endereço que o catálogo conhece (site, extras,
/// instalador, jogo web) ou a própria central.
#[tauri::command]
async fn abrir_link(estado: State<'_, Estado>, url: String) -> Result<(), Falha> {
    let u = so_https(&url)?;
    let conhecido = u.as_str().starts_with(&format!("{API_BASE}/"))
        || u.as_str() == API_BASE
        || estado.catalogo.lock().ok().and_then(|c| c.clone()).map_or(false, |c| {
            c.itens().any(|j| {
                [j.site_url.as_deref(), j.download_url.as_deref(), j.jogar_url.as_deref()]
                    .into_iter()
                    .flatten()
                    .chain(j.extras.iter().map(|e| e.url.as_str()))
                    .any(|x| x == url)
            })
        });
    if !conhecido {
        return Err(Falha::new("invalido", "Endereço fora do catálogo."));
    }
    let alvo = PathBuf::from(u.as_str());
    tauri::async_runtime::spawn_blocking(move || sistema::executar(&alvo, None, None, false))
        .await
        .map_err(interno)?
        .map_err(|_| Falha::new("abrir", "Não consegui abrir o navegador."))?;
    Ok(())
}

/* ---------------- o próprio launcher ---------------- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NovaVersao {
    versao: String,
    notas: Option<String>,
}

/// Procura versão nova no feed assinado (`/api/games/launcher/atualizacao.json`).
#[tauri::command]
async fn procurar_atualizacao(app: AppHandle, estado: State<'_, Estado>) -> Result<Option<NovaVersao>, Falha> {
    if sistema::executando_em_msix() {
        return Ok(None);
    }
    let achou = app
        .updater()
        .map_err(interno)?
        .check()
        .await
        .map_err(|e| Falha::new("rede", format!("Não consegui procurar versão nova do launcher: {e}")))?;
    let r = achou.as_ref().map(|u| NovaVersao { versao: u.version.clone(), notas: u.body.clone() });
    *estado.atualizacao.lock().map_err(|_| Falha::new("interno", "estado travado"))? = achou;
    Ok(r)
}

/// Baixa, confere a assinatura e instala quieto: o instalador fecha este
/// launcher, troca os arquivos e abre a versão nova sozinho (/S /R).
#[tauri::command]
async fn aplicar_atualizacao(app: AppHandle, estado: State<'_, Estado>) -> Result<(), Falha> {
    if sistema::executando_em_msix() {
        return Err(Falha::new("msix", "Esta instalação é atualizada pela Microsoft Store ou pelo App Installer."));
    }
    // trocar o executável agora mataria o download de um jogo no meio
    if estado.ocupados.lock().map(|o| !o.is_empty()).unwrap_or(true) {
        return Err(Falha::new("ocupado", "Termine as instalações em andamento antes de atualizar o launcher."));
    }
    let update = estado
        .atualizacao
        .lock()
        .ok()
        .and_then(|mut a| a.take())
        .ok_or_else(|| Falha::new("invalido", "Nenhuma versão nova encontrada."))?;
    let _ocupado = Ocupado::pegar(&estado, "launcher")?;
    let (app2, mut baixado, mut ultimo) = (app.clone(), 0u64, Instant::now());
    update
        .download_and_install(
            move |n, total| {
                baixado += n as u64;
                if ultimo.elapsed() >= Duration::from_millis(150) {
                    emitir(&app2, "launcher", "baixando", baixado, total, 0);
                    ultimo = Instant::now();
                }
            },
            || {},
        )
        .await
        .map_err(|e| Falha::new("rede", format!("A atualização do launcher falhou: {e}")))?;
    // no Windows não chega aqui: o instalador assume e o launcher reabre sozinho
    app.restart()
}

/// Plano B (feed sem assinatura, updater com erro): baixa o instalador do
/// catálogo, confere o SHA-256 e abre com a janela dele.
#[tauri::command]
async fn atualizar_launcher(app: AppHandle, estado: State<'_, Estado>) -> Result<(), Falha> {
    if sistema::executando_em_msix() {
        return Err(Falha::new("msix", "Esta instalação é atualizada pela Microsoft Store ou pelo App Installer."));
    }
    let info = estado
        .catalogo
        .lock()
        .ok()
        .and_then(|c| c.as_ref().and_then(|c| c.launcher.clone()))
        .ok_or_else(|| Falha::new("invalido", "Nenhuma versão nova publicada."))?;
    so_https(&info.url)?;
    // sem hash não se troca o próprio executável
    let sha = info
        .sha256
        .clone()
        .ok_or_else(|| Falha::new("integridade", "A versão publicada não tem SHA-256; atualize pelo site."))?;
    let _ocupado = Ocupado::pegar(&estado, "launcher")?;
    let destino = pasta_dados(&app)?
        .join("downloads")
        .join(format!(
            "FirawynixCenter-Setup-{}.{}",
            info.versao.replace(|c: char| !c.is_ascii_alphanumeric() && c != '.', ""),
            if cfg!(target_os = "linux") { "AppImage" } else { "exe" }
        ));
    download::baixar(&app, &estado, "launcher", &info.url, &destino, info.tamanho, Some(&sha))
        .await
        .map_err(Falha::from)?;
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissoes = std::fs::metadata(&destino).map_err(interno)?.permissions();
        permissoes.set_mode(permissoes.mode() | 0o755);
        std::fs::set_permissions(&destino, permissoes).map_err(interno)?;
    }
    let alvo = destino.clone();
    tauri::async_runtime::spawn_blocking(move || sistema::executar(&alvo, None, None, false))
        .await
        .map_err(interno)?
        .map_err(|e| match e {
            ErroExec::Cancelado => Falha::new("uac", "Atualização cancelada."),
            ErroExec::Outro(m) => Falha::new("instalador", format!("Não consegui abrir o instalador: {m}")),
        })?;
    app.exit(0);
    Ok(())
}

/// A 1.0.x ("Firawynix Games") guardava os dados em outra pasta. Traz o que
/// importa (jogos localizados à mão, catálogo) e apaga a pasta velha inteira.
fn migrar_dados_antigos(app: &AppHandle) {
    let Ok(nova) = app.path().app_local_data_dir() else { return };
    let Some(velha) = nova.parent().map(|p| p.join("br.com.firawynix.games")) else { return };
    if velha == nova || !velha.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(&nova);
    for nome in [LOCAIS, "catalogo.json"] {
        if !nova.join(nome).exists() {
            let _ = std::fs::copy(velha.join(nome), nova.join(nome));
        }
    }
    let _ = std::fs::remove_dir_all(&velha);
}

pub fn run() {
    let http = reqwest::Client::builder()
        .user_agent(format!(
            "FirawynixCenter/{} ({})",
            env!("CARGO_PKG_VERSION"),
            if cfg!(target_os = "linux") { "Linux" } else { "Windows" }
        ))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("cliente HTTP");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Estado {
            http,
            catalogo: Mutex::new(None),
            origem: Mutex::new("embutido"),
            ocupados: Mutex::new(HashSet::new()),
            cancelados: Mutex::new(HashSet::new()),
            pastas_escolhidas: Mutex::new(HashMap::new()),
            atualizacao: Mutex::new(None),
        })
        .setup(|app| {
            migrar_dados_antigos(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            catalogo,
            estados,
            preparar_instalacao,
            escolher_pasta,
            instalar,
            cancelar,
            criar_atalhos,
            jogar,
            jogar_web,
            plano_desinstalar,
            desinstalar,
            localizar,
            esquecer,
            abrir_pasta,
            abrir_link,
            procurar_atualizacao,
            aplicar_atualizacao,
            atualizar_launcher,
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o Firawynix Center");
}
