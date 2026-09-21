import { invoke } from '@tauri-apps/api/core';

/**
 * Ponte com o Rust (src-tauri/src/lib.rs). Os tipos espelham o catálogo da
 * central (/api/games) — o mesmo JSON que o site lê, mais os `projetos`.
 */

export type TipoJogo = 'download' | 'web';
export type Categoria = 'jogo' | 'projeto';
/** As três listas do launcher: jogos, projetos ou tudo misturado. */
export type Aba = 'jogos' | 'projetos' | 'todos';

export interface LinkExtra {
  rotulo: string;
  url: string;
}

export interface Jogo {
  id: string;
  slug: string;
  nome: string;
  subtitulo: string;
  descricao: string;
  tipo: TipoJogo;
  categoria: Categoria;
  generos: string[];
  cor: string;
  capa: string | null;
  banner: string | null;
  icone: string | null;
  downloadUrl: string | null;
  downloadTamanho: number | null;
  downloadSha256: string | null;
  versao: string | null;
  winChave: string | null;
  winExe: string | null;
  winCaminhos: string[];
  winInstalador: 'manual' | 'inno' | 'nsis' | 'msi' | 'manifesto';
  instaladoTamanho: number | null;
  manifestoUrl: string | null;
  winLimpeza: string[];
  /** O catálogo não oferece binário nativo deste item para o sistema atual. */
  disponivel: boolean;
  extras: LinkExtra[];
  jogarUrl: string | null;
  siteUrl: string | null;
  destaque: boolean;
  ordem: number;
}

export interface LauncherInfo {
  versao: string;
  tamanho: number | null;
  sha256: string | null;
  notas: string;
  publicadoEm: string | null;
  url: string;
}

export interface Catalogo {
  jogos: Jogo[];
  projetos: Jogo[];
  launcher: LauncherInfo | null;
  origem: 'rede' | 'cache' | 'embutido';
  aviso: string | null;
  versaoApp: string;
  atualizacao: boolean;
  base: string;
  plataforma: 'windows' | 'linux';
}

export interface EstadoJogo {
  slug: string;
  instalado: boolean;
  exe: string | null;
  pasta: string | null;
  versaoInstalada: string | null;
  desinstalavel: boolean;
  origem: 'registro' | 'pasta' | 'caminho' | 'localizado' | null;
  /** instalação por manifesto que parou no meio */
  incompleto: boolean;
}

export type Fase = 'baixando' | 'verificando' | 'instalando' | 'desinstalando' | 'concluido';

export interface Progresso {
  slug: string;
  fase: Fase;
  baixado: number;
  total: number | null;
  velocidade: number;
  /** instalação sem janela: na fase "instalando", baixado/total = bytes na pasta / tamanho instalado */
  silencioso: boolean;
  /** instalação por manifesto: arquivos prontos / total */
  arquivos: number | null;
  arquivosTotal: number | null;
}

/** O que está acontecendo com o item agora — decide o texto do botão. */
export type Acao = 'instalar' | 'jogar' | 'desinstalar';

export interface PlanoInstalacao {
  pasta: string | null;
  podeEscolher: boolean;
  livre: number | null;
  necessario: number | null;
  silencioso: boolean;
}

export interface PlanoDesinstalar {
  pasta: string | null;
  silencioso: boolean;
  precisaAdmin: boolean;
  apagaPasta: boolean;
  completa: string[];
  atalhos: string[];
  avisos: string[];
}

export interface ResultadoJogar {
  atualizado: boolean;
  aviso: string | null;
}

export interface ResultadoDesinstalar {
  estado: EstadoJogo;
  restou: string[];
}

export interface NovaVersao {
  versao: string;
  notas: string | null;
}

export interface Falha {
  codigo: string;
  mensagem: string;
}

export const cmd = {
  catalogo: () => invoke<Catalogo>('catalogo'),
  estados: () => invoke<EstadoJogo[]>('estados'),
  prepararInstalacao: (slug: string) => invoke<PlanoInstalacao>('preparar_instalacao', { slug }),
  escolherPasta: (slug: string) => invoke<PlanoInstalacao | null>('escolher_pasta', { slug }),
  instalar: (slug: string, area: boolean, menu: boolean) => invoke<EstadoJogo>('instalar', { slug, area, menu }),
  cancelar: (slug: string) => invoke<void>('cancelar', { slug }),
  criarAtalhos: (slug: string, area: boolean, menu: boolean) => invoke<void>('criar_atalhos', { slug, area, menu }),
  jogar: (slug: string, pular = false) => invoke<ResultadoJogar>('jogar', { slug, pular }),
  jogarWeb: (slug: string) => invoke<void>('jogar_web', { slug }),
  planoDesinstalar: (slug: string) => invoke<PlanoDesinstalar>('plano_desinstalar', { slug }),
  desinstalar: (slug: string, completo: boolean) => invoke<ResultadoDesinstalar>('desinstalar', { slug, completo }),
  localizar: (slug: string) => invoke<EstadoJogo | null>('localizar', { slug }),
  esquecer: (slug: string) => invoke<EstadoJogo>('esquecer', { slug }),
  abrirPasta: (slug: string) => invoke<void>('abrir_pasta', { slug }),
  abrirLink: (url: string) => invoke<void>('abrir_link', { url }),
  procurarAtualizacao: () => invoke<NovaVersao | null>('procurar_atualizacao'),
  aplicarAtualizacao: () => invoke<void>('aplicar_atualizacao'),
  atualizarLauncher: () => invoke<void>('atualizar_launcher'),
};

export function falhaDe(erro: unknown): Falha {
  if (erro && typeof erro === 'object' && 'mensagem' in erro) return erro as Falha;
  return { codigo: 'erro', mensagem: String(erro) };
}

/** O que a pessoa baixa/ocupa: por manifesto é o jogo inteiro, senão o instalador. */
export function tamanhoDoBotao(j: Jogo): number | null {
  return j.winInstalador === 'manifesto' ? j.instaladoTamanho : j.downloadTamanho;
}

/** Jogo "joga", projeto "abre". */
export const verbo = (j: Jogo): string => (j.categoria === 'projeto' ? 'ABRIR' : 'JOGAR');

export function daAba(j: Jogo, aba: Aba): boolean {
  if (aba === 'todos') return true;
  return aba === 'projetos' ? j.categoria === 'projeto' : j.categoria !== 'projeto';
}

/** "32,1 GB" — vírgula decimal, como se lê no Brasil. */
const decimal = (n: number) => n.toFixed(1).replace('.', ',');

export function formatarTamanho(bytes: number | null | undefined): string | null {
  if (!bytes || bytes <= 0) return null;
  const mb = bytes / 1024 ** 2;
  if (mb < 1) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (mb < 1024) return `${mb < 10 ? decimal(mb) : Math.round(mb)} MB`;
  return `${decimal(mb / 1024)} GB`;
}

export function tempoRestante(restante: number, velocidade: number): string | null {
  if (velocidade <= 0 || restante <= 0) return null;
  const s = restante / velocidade;
  if (s < 60) return 'menos de 1 min';
  if (s < 3600) return `${Math.ceil(s / 60)} min`;
  return `${Math.floor(s / 3600)} h ${Math.ceil((s % 3600) / 60)} min`;
}
