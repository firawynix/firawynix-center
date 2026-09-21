//! Download de instalador com retomada, conferência de tamanho/SHA-256 e
//! progresso para a UI. O MU tem ~650 MB: queda de conexão no meio não pode
//! obrigar a baixar tudo de novo, então o arquivo parcial (.part) fica e o
//! próximo pedido continua com `Range`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

use crate::catalogo::e_desafio;
use crate::Estado;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progresso {
    pub slug: String,
    /// baixando | verificando | instalando | concluido
    pub fase: &'static str,
    pub baixado: u64,
    pub total: Option<u64>,
    /// bytes por segundo (média dos últimos instantes)
    pub velocidade: u64,
    /// instalação sem janela: na fase "instalando", baixado/total viram
    /// bytes gravados na pasta / tamanho instalado
    pub silencioso: bool,
    /// instalação por manifesto: arquivos prontos / total de arquivos
    pub arquivos: Option<u64>,
    pub arquivos_total: Option<u64>,
}

pub fn emitir(app: &AppHandle, slug: &str, fase: &'static str, baixado: u64, total: Option<u64>, velocidade: u64) {
    let _ = app.emit(
        "progresso",
        Progresso {
            slug: slug.to_string(),
            fase,
            baixado,
            total,
            velocidade,
            silencioso: false,
            arquivos: None,
            arquivos_total: None,
        },
    );
}

pub fn emitir_manifesto(app: &AppHandle, slug: &str, baixado: u64, total: u64, velocidade: u64, arquivos: u64, arquivos_total: u64) {
    let _ = app.emit(
        "progresso",
        Progresso {
            slug: slug.to_string(),
            fase: "baixando",
            baixado,
            total: Some(total),
            velocidade,
            silencioso: false,
            arquivos: Some(arquivos),
            arquivos_total: Some(arquivos_total),
        },
    );
}

pub fn emitir_instalacao(app: &AppHandle, slug: &str, gravado: u64, total: Option<u64>) {
    let _ = app.emit(
        "progresso",
        Progresso {
            slug: slug.to_string(),
            fase: "instalando",
            baixado: gravado,
            total,
            velocidade: 0,
            silencioso: true,
            arquivos: None,
            arquivos_total: None,
        },
    );
}

pub enum ErroDownload {
    Desafio,
    Status(u16),
    Rede(String),
    Disco(String),
    Integridade(String),
    Cancelado,
}

fn hash_arquivo(caminho: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(caminho)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

async fn hash_async(caminho: PathBuf) -> Result<String, ErroDownload> {
    tauri::async_runtime::spawn_blocking(move || hash_arquivo(&caminho))
        .await
        .map_err(|e| ErroDownload::Disco(e.to_string()))?
        .map_err(|e| ErroDownload::Disco(e.to_string()))
}

/// Confere o arquivo pronto contra o catálogo. Sem SHA-256 no catálogo, fica
/// só o tamanho — melhor que nada, e o painel avisa que o hash protege mais.
async fn confere(caminho: &Path, tamanho: Option<u64>, sha: Option<&str>) -> Result<bool, ErroDownload> {
    let meta = tokio::fs::metadata(caminho).await.map_err(|e| ErroDownload::Disco(e.to_string()))?;
    if let Some(t) = tamanho {
        if meta.len() != t {
            return Ok(false);
        }
    }
    if let Some(esperado) = sha {
        let obtido = hash_async(caminho.to_path_buf()).await?;
        return Ok(obtido.eq_ignore_ascii_case(esperado));
    }
    Ok(true)
}

fn foi_cancelado(estado: &Estado, slug: &str) -> bool {
    estado.cancelados.lock().map(|mut c| c.remove(slug)).unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
pub async fn baixar(
    app: &AppHandle,
    estado: &Estado,
    slug: &str,
    url: &str,
    destino: &Path,
    tamanho: Option<u64>,
    sha: Option<&str>,
) -> Result<(), ErroDownload> {
    // já baixado antes (instalação cancelada no meio, por exemplo)?
    if destino.is_file() {
        emitir(app, slug, "verificando", 0, tamanho, 0);
        if confere(destino, tamanho, sha).await? {
            return Ok(());
        }
        let _ = tokio::fs::remove_file(destino).await;
    }
    if let Some(pasta) = destino.parent() {
        tokio::fs::create_dir_all(pasta).await.map_err(|e| ErroDownload::Disco(e.to_string()))?;
    }

    let parcial = destino.with_extension("part");
    let mut inicio = tokio::fs::metadata(&parcial).await.map(|m| m.len()).unwrap_or(0);
    if tamanho.map_or(false, |t| inicio > t) {
        // parcial maior que o arquivo inteiro: sobra de outra versão
        inicio = 0;
    }

    let mut pedido = estado.http.get(url);
    if inicio > 0 {
        pedido = pedido.header(reqwest::header::RANGE, format!("bytes={inicio}-"));
    }
    let resposta = pedido.send().await.map_err(|e| ErroDownload::Rede(e.to_string()))?;
    if e_desafio(&resposta) {
        return Err(ErroDownload::Desafio);
    }

    let status = resposta.status();
    let continuar = match status.as_u16() {
        206 => true,
        200 => {
            inicio = 0;
            false
        }
        416 if tamanho == Some(inicio) => {
            // o parcial já estava completo
            tokio::fs::rename(&parcial, destino).await.map_err(|e| ErroDownload::Disco(e.to_string()))?;
            return verificar_final(app, slug, destino, tamanho, sha).await;
        }
        416 => {
            let _ = tokio::fs::remove_file(&parcial).await;
            return Err(ErroDownload::Rede("o download anterior estava inconsistente; tente de novo".into()));
        }
        s => return Err(ErroDownload::Status(s)),
    };

    let total = resposta.content_length().map(|n| n + inicio).or(tamanho);
    let mut arquivo = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(continuar)
        .truncate(!continuar)
        .open(&parcial)
        .await
        .map_err(|e| ErroDownload::Disco(e.to_string()))?;

    let mut baixado = inicio;
    let mut fluxo = resposta.bytes_stream();
    let mut ultimo_aviso = Instant::now() - Duration::from_secs(1);
    let mut marca_tempo = Instant::now();
    let mut marca_bytes = baixado;
    let mut velocidade = 0u64;

    loop {
        // conexão que para de mandar bytes sem cair: desiste em 60 s (o .part fica)
        let parte = match tokio::time::timeout(Duration::from_secs(60), fluxo.next()).await {
            Err(_) => return Err(ErroDownload::Rede("o servidor parou de responder".into())),
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(ErroDownload::Rede(e.to_string())),
            Ok(Some(Ok(p))) => p,
        };
        arquivo.write_all(&parte).await.map_err(|e| ErroDownload::Disco(e.to_string()))?;
        baixado += parte.len() as u64;

        if foi_cancelado(estado, slug) {
            let _ = arquivo.flush().await;
            return Err(ErroDownload::Cancelado);
        }

        let agora = Instant::now();
        let janela = agora.duration_since(marca_tempo);
        if janela >= Duration::from_millis(1000) {
            velocidade = ((baixado - marca_bytes) as f64 / janela.as_secs_f64()) as u64;
            marca_tempo = agora;
            marca_bytes = baixado;
        }
        if agora.duration_since(ultimo_aviso) >= Duration::from_millis(150) {
            emitir(app, slug, "baixando", baixado, total, velocidade);
            ultimo_aviso = agora;
        }
    }
    arquivo.flush().await.map_err(|e| ErroDownload::Disco(e.to_string()))?;
    drop(arquivo);
    emitir(app, slug, "baixando", baixado, total, velocidade);

    tokio::fs::rename(&parcial, destino).await.map_err(|e| ErroDownload::Disco(e.to_string()))?;
    verificar_final(app, slug, destino, tamanho, sha).await
}

async fn verificar_final(
    app: &AppHandle,
    slug: &str,
    destino: &Path,
    tamanho: Option<u64>,
    sha: Option<&str>,
) -> Result<(), ErroDownload> {
    emitir(app, slug, "verificando", tamanho.unwrap_or(0), tamanho, 0);
    if confere(destino, tamanho, sha).await? {
        Ok(())
    } else {
        // arquivo errado não fica: o próximo clique baixa do zero
        let _ = tokio::fs::remove_file(destino).await;
        Err(ErroDownload::Integridade(
            "o arquivo baixado não bate com o publicado (tamanho ou SHA-256)".into(),
        ))
    }
}
