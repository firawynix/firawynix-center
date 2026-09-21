//! Instalação e ATUALIZAÇÃO por manifesto: o jogo chega arquivo por arquivo a
//! partir de uma lista `files[{path, url, sha256, size}]` (+ `launcher` opcional
//! e `delete` com o que saiu), o formato do servidor de atualização do Aion.
//! Nenhum instalador roda, então nada abre na tela — e o total é conhecido
//! antes do primeiro byte.
//!
//! - instalar: arquivo que já está na pasta com o tamanho certo fica (retoma
//!   instalação interrompida, inclusive a que o launcher do jogo começou);
//! - atualizar (a cada "Jogar"): compara o manifesto novo com o SHA-256 que
//!   ficou gravado na marca do fim — só baixa o que o servidor mudou, sem reler
//!   34 GB do disco;
//! - cada arquivo baixa em `.fwpart`, com `Range` para continuar e SHA-256
//!   conferido antes de ir para o lugar; 4 ao mesmo tempo, um placar só;
//! - todo arquivo tem de vir do MESMO servidor do manifesto (manifesto adulterado
//!   não manda baixar de outro lugar) e cair DENTRO da pasta do jogo.
//!
//! `.firawynix-instalando` marca a instalação em andamento; `.firawynix-instalado`
//! (com o índice) marca a que terminou.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::catalogo::e_desafio;
use crate::download::emitir_manifesto;
use crate::{sistema, Estado, Falha};

pub const MARCA: &str = ".firawynix-instalado";
pub const EM_ANDAMENTO: &str = ".firawynix-instalando";
const PARALELO: usize = 4;
/// folga de disco além do que falta baixar
const FOLGA: u64 = 512 * 1024 * 1024;
/// SHA-256 de zero bytes
const SHA_VAZIO: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[derive(Deserialize)]
struct Manifesto {
    #[serde(rename = "generatedAt", default)]
    gerado: Option<String>,
    launcher: Option<ArqLauncher>,
    files: Vec<Arquivo>,
    /// arquivos que saíram do jogo: a atualização apaga
    #[serde(default)]
    delete: Vec<String>,
}

#[derive(Deserialize)]
struct ArqLauncher {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Deserialize, Clone)]
struct Arquivo {
    path: String,
    url: String,
    sha256: String,
    size: u64,
}

/// A marca do fim. O `indice` (caminho -> SHA-256) é o que a atualização compara;
/// marca da 1.0.2, sem índice, continua valendo (o tamanho decide na 1ª vez).
#[derive(Serialize, Deserialize, Default)]
struct Marca {
    #[serde(default)]
    manifesto: String,
    #[serde(default)]
    gerado: Option<String>,
    #[serde(default)]
    arquivos: u64,
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    indice: HashMap<String, String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    Instalar,
    /// ao clicar em Jogar: sem rede não é erro, o jogo abre sem conferir
    Atualizar,
}

pub struct Resultado {
    pub baixados: u64,
    pub apagados: u64,
}

/// `bin64/aion.bin` -> `bin64\aion.bin`; recusa absoluto, unidade, `..` e `.`.
fn caminho_seguro(rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('/', "\\");
    if rel.is_empty() || rel.starts_with('\\') || rel.contains(':') {
        return None;
    }
    if rel.split('\\').any(|p| p.is_empty() || p == "." || p == "..") {
        return None;
    }
    Some(PathBuf::from(rel))
}

fn sha_valido(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// "32,1 GB" — vírgula decimal, como a UI mostra.
pub fn legivel(bytes: u64) -> String {
    let gb = bytes as f64 / 1024f64.powi(3);
    if gb >= 1.0 {
        format!("{gb:.1} GB").replace('.', ",")
    } else {
        format!("{:.0} MB", bytes as f64 / 1024f64.powi(2))
    }
}

fn parcial(destino: &Path) -> PathBuf {
    let mut nome = destino.as_os_str().to_owned();
    nome.push(".fwpart");
    PathBuf::from(nome)
}

fn cancelado(estado: &Estado, slug: &str) -> bool {
    estado.cancelados.lock().map(|c| c.contains(slug)).unwrap_or(false)
}

fn pausado() -> Falha {
    Falha::new("cancelado", "Download pausado. Clique de novo para seguir de onde parou.")
}

fn disco(e: std::io::Error) -> Falha {
    Falha::new("disco", format!("Não consegui gravar no disco: {e}"))
}

fn ler_marca(pasta: &Path) -> Option<Marca> {
    let texto = std::fs::read_to_string(pasta.join(MARCA)).ok()?;
    serde_json::from_str(&texto).ok()
}

async fn gravar_marca(pasta: &Path, marca: &Marca) -> Result<(), Falha> {
    let texto = serde_json::to_string(marca).map_err(|e| Falha::new("interno", e.to_string()))?;
    tokio::fs::write(pasta.join(MARCA), texto).await.map_err(disco)
}

/// Um arquivo. `feito` é o placar geral: soma o que chega e desconta se o
/// arquivo precisar recomeçar.
async fn baixar_arquivo(
    http: &reqwest::Client,
    estado: &Estado,
    slug: &str,
    destino: &Path,
    a: &Arquivo,
    feito: &AtomicU64,
    parar: &AtomicBool,
) -> Result<(), Falha> {
    if parar.load(Ordering::Relaxed) || cancelado(estado, slug) {
        return Err(pausado());
    }
    if let Some(p) = destino.parent() {
        tokio::fs::create_dir_all(p).await.map_err(disco)?;
    }
    if a.size == 0 {
        // vazio: nada a baixar — e nenhum .fwpart para renomear depois
        if !a.sha256.eq_ignore_ascii_case(SHA_VAZIO) {
            return Err(Falha::new("rede", format!("{} tem 0 bytes no manifesto mas o SHA-256 não é de arquivo vazio.", a.path)));
        }
        return tokio::fs::write(destino, b"").await.map_err(disco);
    }
    let part = parcial(destino);

    // o que já estava no .fwpart entra no hash (e já está no placar)
    let mut hasher = Sha256::new();
    let mut inicio = 0u64;
    if let Ok(mut f) = tokio::fs::File::open(&part).await {
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).await.map_err(disco)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            inicio += n as u64;
        }
    }
    if inicio > a.size {
        let _ = tokio::fs::remove_file(&part).await;
        feito.fetch_sub(a.size.min(inicio), Ordering::Relaxed);
        hasher = Sha256::new();
        inicio = 0;
    }
    let mut contado = inicio;

    if inicio < a.size {
        let mut pedido = http.get(&a.url);
        if inicio > 0 {
            pedido = pedido.header(reqwest::header::RANGE, format!("bytes={inicio}-"));
        }
        let resposta = pedido
            .send()
            .await
            .map_err(|e| Falha::new("rede", format!("Falha de conexão em {}: {e}", a.path)))?;
        if e_desafio(&resposta) {
            return Err(Falha::new("desafio", "O servidor do jogo pediu verificação de navegador."));
        }
        let continuar = match resposta.status().as_u16() {
            206 => true,
            200 => {
                // o servidor ignorou o Range: recomeça este arquivo do zero
                feito.fetch_sub(contado, Ordering::Relaxed);
                contado = 0;
                hasher = Sha256::new();
                false
            }
            s => return Err(Falha::new("rede", format!("O servidor respondeu {s} para {}.", a.path))),
        };
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(continuar)
            .truncate(!continuar)
            .open(&part)
            .await
            .map_err(disco)?;
        let mut fluxo = resposta.bytes_stream();
        loop {
            let pedaco = match tokio::time::timeout(Duration::from_secs(60), fluxo.next()).await {
                Err(_) => return Err(Falha::new("rede", "O servidor do jogo parou de responder.")),
                Ok(None) => break,
                Ok(Some(Err(e))) => return Err(Falha::new("rede", format!("Falha de conexão: {e}"))),
                Ok(Some(Ok(p))) => p,
            };
            f.write_all(&pedaco).await.map_err(disco)?;
            hasher.update(&pedaco);
            contado += pedaco.len() as u64;
            feito.fetch_add(pedaco.len() as u64, Ordering::Relaxed);
            if parar.load(Ordering::Relaxed) || cancelado(estado, slug) {
                let _ = f.flush().await;
                return Err(pausado());
            }
        }
        f.flush().await.map_err(disco)?;
    }

    if !hex::encode(hasher.finalize()).eq_ignore_ascii_case(&a.sha256) {
        let _ = tokio::fs::remove_file(&part).await;
        feito.fetch_sub(contado, Ordering::Relaxed);
        return Err(Falha::new("rede", format!("{} chegou corrompido.", a.path)));
    }
    // o destino pode existir (versão velha, tamanho errado): sai antes do rename
    let _ = tokio::fs::remove_file(destino).await;
    tokio::fs::rename(&part, destino).await.map_err(disco)?;
    Ok(())
}

/// Até 3 tentativas para falha de rede (queda, arquivo corrompido no caminho).
async fn baixar_com_retentativa(
    http: &reqwest::Client,
    estado: &Estado,
    slug: &str,
    destino: &Path,
    a: &Arquivo,
    feito: &AtomicU64,
    parar: &AtomicBool,
) -> Result<(), Falha> {
    let mut tentativa = 0;
    loop {
        match baixar_arquivo(http, estado, slug, destino, a, feito, parar).await {
            Err(f) if f.codigo == "rede" && tentativa < 2 && !parar.load(Ordering::Relaxed) => {
                tentativa += 1;
                tokio::time::sleep(Duration::from_secs(2 * tentativa)).await;
            }
            r => return r,
        }
    }
}

async fn buscar(estado: &Estado, url: &str, modo: Modo) -> Result<Manifesto, Falha> {
    // na atualização, qualquer falha para buscar a lista vira "sem_rede": o jogo abre assim mesmo
    let atualizar = modo == Modo::Atualizar;
    let codigo = if atualizar { "sem_rede" } else { "rede" };
    let r = estado
        .http
        .get(url)
        .timeout(Duration::from_secs(if atualizar { 25 } else { 90 }))
        .send()
        .await
        .map_err(|e| Falha::new(codigo, format!("Não consegui buscar a lista de arquivos do jogo: {e}")))?;
    if e_desafio(&r) {
        return Err(Falha::new(if atualizar { "sem_rede" } else { "desafio" }, "O servidor do jogo pediu verificação de navegador."));
    }
    if !r.status().is_success() {
        return Err(Falha::new(codigo, format!("A lista de arquivos do jogo respondeu {}.", r.status())));
    }
    r.json()
        .await
        .map_err(|e| Falha::new(codigo, format!("A lista de arquivos do jogo veio inválida: {e}")))
}

/// Deixa a pasta igual ao manifesto. Em `Instalar`, o placar mostra o jogo
/// inteiro; em `Atualizar`, só o que mudou.
pub async fn sincronizar(
    app: &AppHandle,
    estado: &Estado,
    slug: &str,
    manifesto_url: &str,
    pasta: &Path,
    exe_launcher: Option<&str>,
    modo: Modo,
) -> Result<Resultado, Falha> {
    let base = url::Url::parse(manifesto_url).map_err(|_| Falha::new("invalido", "Link do manifesto inválido."))?;
    let host = base.host_str().unwrap_or_default().to_string();

    let m = buscar(estado, manifesto_url, modo).await?;
    let gerado = m.gerado.clone();
    let mut arquivos = m.files;
    // o launcher do próprio jogo (ex.: AionH4KLauncher.exe) vai com o nome do Executável
    if let (Some(l), Some(exe)) = (m.launcher, exe_launcher) {
        arquivos.push(Arquivo { path: exe.to_string(), url: l.url, sha256: l.sha256, size: l.size });
    }

    let mut itens = Vec::with_capacity(arquivos.len());
    for a in arquivos {
        let rel = caminho_seguro(&a.path)
            .ok_or_else(|| Falha::new("invalido", format!("Caminho recusado no manifesto: {}", a.path)))?;
        let u = url::Url::parse(&a.url).map_err(|_| Falha::new("invalido", format!("Endereço inválido no manifesto: {}", a.url)))?;
        if u.scheme() != "https" || u.host_str() != Some(host.as_str()) {
            return Err(Falha::new("invalido", format!("O manifesto aponta para fora do servidor do jogo: {}", a.url)));
        }
        if !sha_valido(&a.sha256) {
            return Err(Falha::new("invalido", format!("Arquivo sem SHA-256 válido no manifesto: {}", a.path)));
        }
        let chave = a.path.replace('\\', "/");
        itens.push((pasta.join(rel), chave, a));
    }
    let total: u64 = itens.iter().map(|(_, _, a)| a.size).sum();
    let quantos = itens.len() as u64;

    tokio::fs::create_dir_all(pasta).await.map_err(disco)?;
    let antiga = if modo == Modo::Atualizar { ler_marca(pasta) } else { None };
    if modo == Modo::Instalar {
        // sem esta marca, um download pela metade (sem o .exe ainda) pareceria "não instalado"
        tokio::fs::write(pasta.join(EM_ANDAMENTO), manifesto_url).await.map_err(disco)?;
    }

    let mut indice = HashMap::with_capacity(itens.len());
    let mut pendentes = Vec::new();
    let (mut prontos, mut prontos_bytes, mut parciais) = (0u64, 0u64, 0u64);
    for (destino, chave, a) in itens {
        let md = tokio::fs::metadata(&destino).await.ok().filter(|m| m.is_file());
        let tamanho_certo = md.as_ref().map_or(false, |m| m.len() == a.size);
        let pendente = match antiga.as_ref().and_then(|m| m.indice.get(&chave)) {
            // conferido antes: só baixa se o servidor mudou o arquivo (ou ele sumiu).
            // Tamanho diferente com o mesmo SHA é patch do launcher do jogo — fica.
            Some(sha) => !sha.eq_ignore_ascii_case(&a.sha256) || md.is_none(),
            None => !tamanho_certo,
        };
        indice.insert(chave, a.sha256.to_ascii_lowercase());
        if pendente {
            if let Ok(p) = tokio::fs::metadata(parcial(&destino)).await {
                parciais += p.len().min(a.size);
            }
            pendentes.push((destino, a));
        } else {
            prontos += 1;
            prontos_bytes += a.size;
        }
    }

    let mut apagados = 0u64;
    if modo == Modo::Atualizar {
        for rel in &m.delete {
            if let Some(r) = caminho_seguro(rel) {
                let p = pasta.join(r);
                if p.is_file() && tokio::fs::remove_file(&p).await.is_ok() {
                    apagados += 1;
                }
            }
        }
    }

    let marca = Marca { manifesto: manifesto_url.to_string(), gerado, arquivos: quantos, bytes: total, indice };
    if pendentes.is_empty() {
        let mudou = antiga.as_ref().map_or(true, |a| a.indice != marca.indice || a.gerado != marca.gerado);
        if modo == Modo::Instalar || mudou {
            gravar_marca(pasta, &marca).await?;
        }
        let _ = tokio::fs::remove_file(pasta.join(EM_ANDAMENTO)).await;
        return Ok(Resultado { baixados: 0, apagados });
    }

    let (base_feito, total_barra, feitos_inicio, quantos_barra) = match modo {
        Modo::Instalar => (prontos_bytes + parciais, total, prontos, quantos),
        Modo::Atualizar => (parciais, pendentes.iter().map(|(_, a)| a.size).sum(), 0, pendentes.len() as u64),
    };
    let falta = total_barra.saturating_sub(base_feito);
    emitir_manifesto(app, slug, base_feito, total_barra, 0, feitos_inicio, quantos_barra);
    if let Some(livre) = sistema::espaco_livre(pasta) {
        if livre < falta + FOLGA {
            return Err(Falha::new(
                "espaco",
                format!(
                    "Espaço insuficiente em {}: faltam {} e há {} livres.",
                    pasta.display(),
                    legivel(falta + FOLGA),
                    legivel(livre)
                ),
            ));
        }
    }

    let n_pendentes = pendentes.len() as u64;
    let feito = Arc::new(AtomicU64::new(base_feito));
    let feitos = Arc::new(AtomicU64::new(feitos_inicio));
    let parar = Arc::new(AtomicBool::new(false));
    let fim = Arc::new(AtomicBool::new(false));

    // placar: um aviso a cada 400 ms com a soma dos downloads em paralelo
    let placar = {
        let (app, slug, feito, feitos, fim) = (app.clone(), slug.to_string(), feito.clone(), feitos.clone(), fim.clone());
        tauri::async_runtime::spawn(async move {
            let (mut marca_t, mut marca_b) = (Instant::now(), feito.load(Ordering::Relaxed));
            let mut velocidade = 0u64;
            loop {
                let agora = feito.load(Ordering::Relaxed);
                let janela = marca_t.elapsed();
                if janela >= Duration::from_secs(2) {
                    velocidade = (agora.saturating_sub(marca_b) as f64 / janela.as_secs_f64()) as u64;
                    marca_t = Instant::now();
                    marca_b = agora;
                }
                emitir_manifesto(&app, &slug, agora.min(total_barra), total_barra, velocidade, feitos.load(Ordering::Relaxed), quantos_barra);
                if fim.load(Ordering::Relaxed) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        })
    };

    let http = estado.http.clone();
    let mut fluxo = futures_util::stream::iter(pendentes.into_iter().map(|(destino, a)| {
        let (http, feito, feitos, parar) = (http.clone(), feito.clone(), feitos.clone(), parar.clone());
        async move {
            let r = baixar_com_retentativa(&http, estado, slug, &destino, &a, &feito, &parar).await;
            if r.is_ok() {
                feitos.fetch_add(1, Ordering::Relaxed);
            }
            r
        }
    }))
    .buffer_unordered(PARALELO);

    let mut primeiro_erro: Option<Falha> = None;
    while let Some(r) = fluxo.next().await {
        if let Err(f) = r {
            if primeiro_erro.is_none() {
                // o primeiro erro para os outros downloads; os .fwpart ficam para retomar
                parar.store(true, Ordering::Relaxed);
                primeiro_erro = Some(f);
            }
        }
    }
    drop(fluxo);
    fim.store(true, Ordering::Relaxed);
    let _ = placar.await;
    if let Ok(mut c) = estado.cancelados.lock() {
        c.remove(slug);
    }
    if let Some(f) = primeiro_erro {
        return Err(f);
    }

    gravar_marca(pasta, &marca).await?;
    let _ = tokio::fs::remove_file(pasta.join(EM_ANDAMENTO)).await;
    Ok(Resultado { baixados: n_pendentes, apagados })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn caminho_do_manifesto_nao_sai_da_pasta() {
        assert_eq!(caminho_seguro("bin64/aion.bin"), Some(PathBuf::from(r"bin64\aion.bin")));
        assert_eq!(caminho_seguro("AionH4KLauncher.exe"), Some(PathBuf::from("AionH4KLauncher.exe")));
        assert!(caminho_seguro("../evil.exe").is_none());
        assert!(caminho_seguro("bin64/../../evil.exe").is_none());
        assert!(caminho_seguro("/etc/passwd").is_none());
        assert!(caminho_seguro(r"C:\Windows\evil.exe").is_none());
        assert!(caminho_seguro("a//b").is_none());
        assert!(caminho_seguro("").is_none());
    }

    #[test]
    fn sha_e_tamanho_legivel() {
        assert!(sha_valido(&"a".repeat(64)));
        assert!(!sha_valido("abc"));
        assert!(!sha_valido(&"z".repeat(64)));
        assert_eq!(legivel(34_476_490_119), "32,1 GB");
        assert_eq!(legivel(5_272_310), "5 MB");
        assert_eq!(hex::encode(Sha256::digest(b"")), SHA_VAZIO);
    }

    #[test]
    fn manifesto_do_aion_e_lido() {
        let json = r#"{"schema":1,"generatedAt":"2026-09-10T00:26:42Z","launcher":{"version":"1.4.1","url":"https://x/AionH4KLauncher.exe","sha256":"aa","size":10},
                      "files":[{"path":"bin64/aion.bin","url":"https://x/f","sha256":"bb","size":5}],"delete":["old/x.pak"]}"#;
        let m: Manifesto = serde_json::from_str(json).unwrap();
        assert_eq!(m.files.len(), 1);
        assert_eq!(m.launcher.unwrap().size, 10);
        assert_eq!(m.gerado.as_deref(), Some("2026-09-10T00:26:42Z"));
        assert_eq!(m.delete, vec!["old/x.pak".to_string()]);
    }

    /// O erro do Aion ("os error 2"): arquivo de 0 byte nunca criava o .fwpart e o rename falhava.
    #[test]
    fn arquivo_vazio_sai_sem_download() {
        use std::sync::Mutex;
        let estado = Estado {
            http: reqwest::Client::new(),
            catalogo: Mutex::new(None),
            origem: Mutex::new("rede"),
            ocupados: Default::default(),
            cancelados: Default::default(),
            pastas_escolhidas: Default::default(),
            atualizacao: Mutex::new(None),
        };
        let d = std::env::temp_dir().join(format!("fw-vazio-{}", std::process::id()));
        let destino = d.join("Levels").join("ldf5b").join("path.pak");
        // URL que não resolve: se tentasse baixar, falharia
        let a = Arquivo { path: "Levels/ldf5b/path.pak".into(), url: "https://x.invalid/nada".into(), sha256: SHA_VAZIO.into(), size: 0 };
        let (feito, parar) = (AtomicU64::new(0), AtomicBool::new(false));
        tauri::async_runtime::block_on(baixar_arquivo(&estado.http, &estado, "aion", &destino, &a, &feito, &parar)).unwrap();
        assert_eq!(std::fs::metadata(&destino).unwrap().len(), 0);
        // SHA de arquivo cheio com tamanho 0 = manifesto errado
        let ruim = Arquivo { sha256: "a".repeat(64), ..a };
        assert!(tauri::async_runtime::block_on(baixar_arquivo(&estado.http, &estado, "aion", &destino, &ruim, &feito, &parar)).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn marca_da_versao_antiga_ainda_le() {
        let m: Marca = serde_json::from_str(r#"{"manifesto":"https://x/m.json","gerado":null,"arquivos":2,"bytes":9}"#).unwrap();
        assert!(m.indice.is_empty());
        assert_eq!(m.arquivos, 2);
    }
}
