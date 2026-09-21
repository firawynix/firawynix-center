//! O catálogo da central (GET /api/games): o mesmo JSON que o site lê, mais a
//! lista de `projetos` que só o launcher mostra.
//!
//! Três origens, nesta ordem: a rede; o último catálogo que deu certo (cache em
//! disco); e o catálogo embutido no executável. Assim o launcher abre e deixa
//! jogar o que já está instalado mesmo sem internet ou com a API fora do ar.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::API_BASE;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkExtra {
    pub rotulo: String,
    pub url: String,
}

fn cor_padrao() -> String {
    "#22d3ee".into()
}

fn instalador_padrao() -> String {
    "manual".into()
}

fn categoria_padrao() -> String {
    "jogo".into()
}

fn disponivel_padrao() -> bool {
    true
}

/// Artefato nativo para Linux. Campos Windows continuam no nível principal
/// para que launchers antigos sigam entendendo o mesmo catálogo.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxArtefato {
    pub url: String,
    pub tamanho: Option<u64>,
    pub sha256: Option<String>,
    pub versao: Option<String>,
    pub exe: String,
    #[serde(default)]
    pub caminhos: Vec<String>,
    /// appimage | manual
    #[serde(default = "instalador_padrao")]
    pub instalador: String,
    pub instalado_tamanho: Option<u64>,
    #[serde(default)]
    pub limpeza: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Jogo {
    pub id: String,
    pub slug: String,
    pub nome: String,
    #[serde(default)]
    pub subtitulo: String,
    #[serde(default)]
    pub descricao: String,
    pub tipo: String,
    /// jogo | projeto — catálogo antigo sem o campo = jogo
    #[serde(default = "categoria_padrao")]
    pub categoria: String,
    #[serde(default)]
    pub generos: Vec<String>,
    #[serde(default = "cor_padrao")]
    pub cor: String,
    pub capa: Option<String>,
    pub banner: Option<String>,
    pub icone: Option<String>,
    pub download_url: Option<String>,
    pub download_tamanho: Option<u64>,
    pub download_sha256: Option<String>,
    pub versao: Option<String>,
    pub win_chave: Option<String>,
    pub win_exe: Option<String>,
    #[serde(default)]
    pub win_caminhos: Vec<String>,
    /// manual | inno | nsis | msi | manifesto — catálogo antigo sem o campo = manual
    #[serde(default = "instalador_padrao")]
    pub win_instalador: String,
    /// bytes da pasta instalada: o 100% da barra de instalação silenciosa
    pub instalado_tamanho: Option<u64>,
    /// instalação por manifesto: lista de arquivos que o launcher baixa sozinho
    pub manifesto_url: Option<String>,
    /// o que a desinstalação completa apaga além da pasta: `%APPDATA%\X` ou
    /// `HKCU\Software\X\Y` (validado de novo em `sistema::limpeza`)
    #[serde(default)]
    pub win_limpeza: Vec<String>,
    /// Ausente significa que o projeto ainda é exclusivo do Windows.
    pub linux: Option<LinuxArtefato>,
    /// Calculado pelo launcher para a plataforma atual; servidores antigos não
    /// enviam o campo e, nesse caso, Windows continua disponível.
    #[serde(default = "disponivel_padrao")]
    pub disponivel: bool,
    #[serde(default)]
    pub extras: Vec<LinkExtra>,
    pub jogar_url: Option<String>,
    pub site_url: Option<String>,
    #[serde(default)]
    pub destaque: bool,
    #[serde(default)]
    pub ordem: i32,
}

impl Jogo {
    pub fn e_web(&self) -> bool {
        self.tipo == "web"
    }

    pub fn e_projeto(&self) -> bool {
        self.categoria == "projeto"
    }

    pub fn silencioso(&self) -> bool {
        matches!(self.win_instalador.as_str(), "inno" | "nsis" | "msi")
    }

    pub fn por_manifesto(&self) -> bool {
        self.win_instalador == "manifesto"
    }
}

impl Jogo {
    #[cfg(target_os = "linux")]
    fn selecionar_plataforma(&mut self) {
        if self.e_web() {
            self.disponivel = true;
            return;
        }
        // Os jogos nativos ainda são Windows-only. O Linux recebe somente os
        // utilitários que tenham artefato próprio publicado no catálogo.
        if !self.e_projeto() {
            self.disponivel = false;
            self.download_url = None;
            return;
        }
        let Some(linux) = self.linux.clone() else {
            self.disponivel = false;
            self.download_url = None;
            return;
        };
        self.download_url = Some(absoluto(&linux.url));
        self.download_tamanho = linux.tamanho;
        self.download_sha256 = linux.sha256;
        self.versao = linux.versao;
        self.win_chave = None;
        self.win_exe = Some(linux.exe);
        self.win_caminhos = linux.caminhos;
        self.win_instalador = linux.instalador;
        self.instalado_tamanho = linux.instalado_tamanho;
        self.manifesto_url = None;
        self.win_limpeza = linux.limpeza;
        self.disponivel = true;
    }

    #[cfg(not(target_os = "linux"))]
    fn selecionar_plataforma(&mut self) {
        self.disponivel = true;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherInfo {
    pub versao: String,
    pub tamanho: Option<u64>,
    pub sha256: Option<String>,
    #[serde(default)]
    pub notas: String,
    pub publicado_em: Option<String>,
    pub url: String,
    pub linux: Option<LauncherArtefato>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherArtefato {
    pub tamanho: Option<u64>,
    pub sha256: Option<String>,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalogo {
    pub jogos: Vec<Jogo>,
    /// só o launcher lista: o site da central mostra os jogos
    #[serde(default)]
    pub projetos: Vec<Jogo>,
    pub launcher: Option<LauncherInfo>,
}

/// O que a UI recebe: o catálogo + de onde ele veio + se há launcher novo.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resposta {
    pub jogos: Vec<Jogo>,
    pub projetos: Vec<Jogo>,
    pub launcher: Option<LauncherInfo>,
    pub origem: &'static str,
    pub aviso: Option<String>,
    pub versao_app: String,
    pub atualizacao: bool,
    pub base: String,
    pub plataforma: &'static str,
}

const EMBUTIDO: &str = include_str!("../catalogo-inicial.json");

/// A API devolve imagens e o instalador do launcher como caminho relativo
/// ("/api/games/media/..."); a UI e o download precisam do endereço inteiro.
pub fn absoluto(url: &str) -> String {
    if url.starts_with('/') {
        format!("{API_BASE}{url}")
    } else {
        url.to_string()
    }
}

impl Catalogo {
    pub fn normalizado(mut self) -> Self {
        for j in &mut self.projetos {
            j.categoria = "projeto".into();
        }
        for j in self.jogos.iter_mut().chain(self.projetos.iter_mut()) {
            j.capa = j.capa.take().map(|u| absoluto(&u));
            j.banner = j.banner.take().map(|u| absoluto(&u));
            j.icone = j.icone.take().map(|u| absoluto(&u));
            j.selecionar_plataforma();
        }
        if let Some(l) = &mut self.launcher {
            l.url = absoluto(&l.url);
            if let Some(linux) = &mut l.linux {
                linux.url = absoluto(&linux.url);
            }
            #[cfg(target_os = "linux")]
            if let Some(linux) = l.linux.take() {
                l.tamanho = linux.tamanho;
                l.sha256 = linux.sha256;
                l.url = linux.url;
            }
        }
        self
    }

    /// Jogos e projetos juntos: para o Rust os dois são "itens" iguais.
    pub fn itens(&self) -> impl Iterator<Item = &Jogo> {
        self.jogos.iter().chain(self.projetos.iter())
    }

    pub fn jogo(&self, slug: &str) -> Option<&Jogo> {
        self.itens().find(|j| j.slug == slug)
    }
}

pub enum ErroRede {
    /// A Cloudflare pediu o desafio de navegador: falta a regra de skip.
    Desafio,
    Status(u16),
    Rede(String),
}

impl ErroRede {
    pub fn texto(&self) -> String {
        match self {
            ErroRede::Desafio => "a Cloudflare pediu verificação de navegador".into(),
            ErroRede::Status(s) => format!("o servidor respondeu {s}"),
            ErroRede::Rede(e) => e.clone(),
        }
    }
}

/// 403 com `cf-mitigated: challenge` é a página "Just a moment...", não a API.
pub fn e_desafio(r: &reqwest::Response) -> bool {
    r.status() == reqwest::StatusCode::FORBIDDEN
        && r
            .headers()
            .get("cf-mitigated")
            .map_or(false, |v| v.as_bytes().eq_ignore_ascii_case(b"challenge"))
}

pub async fn buscar(http: &reqwest::Client) -> Result<Catalogo, ErroRede> {
    let r = http
        .get(format!("{API_BASE}/api/games"))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| ErroRede::Rede(e.to_string()))?;
    if e_desafio(&r) {
        return Err(ErroRede::Desafio);
    }
    if !r.status().is_success() {
        return Err(ErroRede::Status(r.status().as_u16()));
    }
    let c: Catalogo = r.json().await.map_err(|e| ErroRede::Rede(format!("resposta inválida: {e}")))?;
    Ok(c.normalizado())
}

pub fn ler_cache(caminho: &Path) -> Option<Catalogo> {
    let texto = std::fs::read_to_string(caminho).ok()?;
    serde_json::from_str(&texto).ok()
}

pub fn gravar_cache(caminho: &Path, c: &Catalogo) {
    if let Some(pasta) = caminho.parent() {
        let _ = std::fs::create_dir_all(pasta);
    }
    if let Ok(texto) = serde_json::to_string(c) {
        let _ = std::fs::write(caminho, texto);
    }
}

pub fn embutido() -> Catalogo {
    serde_json::from_str::<Catalogo>(EMBUTIDO)
        .map(Catalogo::normalizado)
        .unwrap_or(Catalogo { jogos: vec![], projetos: vec![], launcher: None })
}

/// Todos os grupos de dígitos, na ordem: "v26.11.70-firaw.4" -> [26, 11, 70, 4].
/// Só os três primeiros deixava "firaw.4" igual a "firaw.3" e o Kdenlive nunca
/// atualizava pelo launcher (é a mesma regra do launcher do próprio Kdenlive).
fn partes(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u64>().unwrap_or(u64::MAX))
        .collect()
}

/// "1.2.10" > "1.2.9" — comparação numérica, não de texto. Grupo que falta vale
/// zero: "1.0" == "1.0.0".
pub fn versao_maior(nova: &str, atual: &str) -> bool {
    let (a, b) = (partes(nova), partes(atual));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn compara_versao_por_numero() {
        assert!(versao_maior("1.2.10", "1.2.9"));
        assert!(versao_maior("2.0.0", "1.9.9"));
        assert!(versao_maior("v1.4.6", "1.4.5"));
        assert!(!versao_maior("1.0.0", "1.0.0"));
        assert!(!versao_maior("0.9.0", "1.0.0"));
        // sufixo numérico conta (Kdenlive: 26.11.70-firaw.N)
        assert!(versao_maior("26.11.70-firaw.4", "26.11.70-firaw.3"));
        assert!(versao_maior("v26.11.70-firaw.10", "26.11.70-firaw.9"));
        assert!(!versao_maior("26.11.70-firaw.3", "26.11.70-firaw.3"));
        assert!(!versao_maior("26.11.70", "26.11.70-firaw.1"));
        // grupo que falta vale zero
        assert!(!versao_maior("1.0", "1.0.0"));
        assert!(!versao_maior("1.0.0.0", "1.0"));
        assert!(versao_maior("1.0.0.1", "1.0"));
    }

    #[test]
    fn catalogo_embutido_e_valido() {
        let c = embutido();
        assert!(c.jogos.len() >= 4);
        assert!(c.jogos.iter().all(|j| j.e_web()));
        assert!(c.jogos.iter().all(|j| j.download_url.is_none()));
        assert!(c.projetos.iter().all(|p| p.e_projeto()));
    }

    #[test]
    fn catalogo_antigo_sem_projetos_ainda_le() {
        let c: Catalogo = serde_json::from_str(r#"{"jogos":[{"id":"a","slug":"a","nome":"A","tipo":"web"}],"launcher":null}"#).unwrap();
        assert!(c.projetos.is_empty());
        assert_eq!(c.jogos[0].categoria, "jogo");
        assert!(c.jogos[0].win_limpeza.is_empty());
    }

    #[test]
    fn caminho_relativo_vira_absoluto() {
        assert_eq!(absoluto("/api/games/media/x.png"), format!("{API_BASE}/api/games/media/x.png"));
        assert_eq!(absoluto("https://a.b/c.png"), "https://a.b/c.png");
    }
}
