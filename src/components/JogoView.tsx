import { useEffect, useRef, useState } from 'react';
import {
  FiAlertTriangle,
  FiDownload,
  FiExternalLink,
  FiFolder,
  FiGlobe,
  FiLink,
  FiMoreHorizontal,
  FiPause,
  FiPlay,
  FiSearch,
  FiTrash2,
  FiX,
} from 'react-icons/fi';
import { FaMicrosoft } from 'react-icons/fa6';
import { Acao, EstadoJogo, Falha, formatarTamanho, Jogo, Progresso, tamanhoDoBotao, tempoRestante, verbo } from '../api';
import GameArt from './GameArt';

export interface Acoes {
  /** abre a janela de instalar (pasta e atalhos) */
  instalar: (slug: string) => void;
  cancelar: (slug: string) => void;
  /** confere atualização e abre; `pular` abre sem conferir */
  jogar: (slug: string, pular?: boolean) => void;
  jogarWeb: (slug: string) => void;
  /** abre a janela de desinstalar */
  desinstalar: (slug: string) => void;
  criarAtalhos: (slug: string) => void;
  localizar: (slug: string) => void;
  esquecer: (slug: string) => void;
  abrirPasta: (slug: string) => void;
  abrirLink: (url: string) => void;
  limparFalha: (slug: string) => void;
}

function textoDoAndamento(acao: Acao, progresso: Progresso | undefined, pct: number, comBarra: boolean): string {
  if (acao === 'desinstalar') return 'Desinstalando...';
  // ao clicar em Jogar, baixar e instalar são a atualização
  const atualizando = acao === 'jogar';
  switch (progresso?.fase) {
    case 'verificando':
      return atualizando ? 'Procurando atualização...' : 'Conferindo o arquivo...';
    case 'baixando':
      return comBarra ? `${atualizando ? 'Atualizando' : 'Baixando'} ${pct}%` : 'Baixando...';
    case 'instalando':
      if (!progresso.silencioso) return 'Conclua o instalador...';
      if (comBarra) return `${atualizando ? 'Atualizando' : 'Instalando'} ${pct}%`;
      return atualizando ? 'Atualizando...' : 'Instalando...';
    default:
      return atualizando ? 'Abrindo...' : 'Preparando...';
  }
}

function BotaoPrincipal({
  jogo,
  estado,
  progresso,
  acao,
  acoes,
}: {
  jogo: Jogo;
  estado?: EstadoJogo;
  progresso?: Progresso;
  acao?: Acao;
  acoes: Acoes;
}) {
  const grande = 'btn-primary min-w-[200px] px-8 py-3 text-base tracking-wide';

  if (jogo.tipo === 'web') {
    return (
      <button type="button" className={grande} onClick={() => acoes.jogarWeb(jogo.slug)}>
        <FiPlay aria-hidden="true" /> {verbo(jogo)}
      </button>
    );
  }

  if (!jogo.disponivel) {
    return (
      <span className="btn-secondary cursor-default px-7 py-3 text-sm opacity-75">
        Ainda não disponível para Linux
      </span>
    );
  }

  if (acao) {
    const baixando = progresso?.fase === 'baixando' && !!progresso.total;
    const silencioso = progresso?.fase === 'instalando' && progresso.silencioso;
    // instalação silenciosa com tamanho conhecido também tem porcentagem de verdade
    const comBarra = baixando || (silencioso && !!progresso?.total);
    const pct = comBarra ? Math.min(99, Math.floor((progresso!.baixado / progresso!.total!) * 100)) : 0;
    const texto = textoDoAndamento(acao, progresso, pct, comBarra);
    const detalhe =
      acao === 'desinstalar'
        ? 'Se o Windows pedir permissão, confirme'
        : baixando
          ? [
              `${formatarTamanho(progresso.baixado) ?? '0'} de ${formatarTamanho(progresso.total) ?? '?'}`,
              progresso.velocidade > 0 ? `${formatarTamanho(progresso.velocidade)}/s` : null,
              tempoRestante(progresso.total! - progresso.baixado, progresso.velocidade),
              progresso.arquivosTotal ? `arquivo ${progresso.arquivos ?? 0} de ${progresso.arquivosTotal}` : null,
            ]
              .filter(Boolean)
              .join(' · ')
          : silencioso
            ? progresso.baixado > 0
              ? `${formatarTamanho(progresso.baixado)}${progresso.total ? ` de ${formatarTamanho(progresso.total)}` : ''}`
              : 'Se o Windows pedir permissão, confirme'
            : acao === 'instalar' && !progresso && formatarTamanho(tamanhoDoBotao(jogo))
              ? `Total: ${formatarTamanho(tamanhoDoBotao(jogo))}`
              : null;
    const pausavel = acao !== 'desinstalar' && (progresso?.fase === 'baixando' || (!progresso && acao === 'instalar'));
    return (
      <div className="flex min-w-[340px] items-center gap-3">
        <div className="flex-1">
          <div className="mb-1.5 flex items-baseline justify-between gap-3">
            <span className="text-sm font-semibold text-white">{texto}</span>
            {detalhe && <span className="font-mono text-[11px] text-slate-400">{detalhe}</span>}
          </div>
          <div className="h-2.5 overflow-hidden rounded-full bg-surface-lighter">
            {comBarra ? (
              <div className="h-full rounded-full bg-brand transition-[width] duration-300" style={{ width: `${pct}%` }} />
            ) : (
              <div className="barra-indeterminada h-full w-full rounded-full" />
            )}
          </div>
        </div>
        {pausavel && (
          <button
            type="button"
            className="btn-secondary px-3 py-2"
            onClick={() => acoes.cancelar(jogo.slug)}
            title="Pausar (continua de onde parou)"
            aria-label="Pausar download"
          >
            <FiPause aria-hidden="true" />
          </button>
        )}
      </div>
    );
  }

  if (estado?.instalado) {
    return (
      <button type="button" className={grande} onClick={() => acoes.jogar(jogo.slug)}>
        <FiPlay aria-hidden="true" /> {verbo(jogo)}
      </button>
    );
  }

  const peso = formatarTamanho(tamanhoDoBotao(jogo));
  return (
    <button type="button" className={grande} onClick={() => acoes.instalar(jogo.slug)}>
      <FiDownload aria-hidden="true" /> {estado?.incompleto ? 'CONTINUAR' : 'INSTALAR'}
      {peso && <span className="font-mono text-xs font-medium opacity-70">{peso}</span>}
    </button>
  );
}

function Menu({ jogo, estado, ocupado, acoes }: { jogo: Jogo; estado?: EstadoJogo; ocupado: boolean; acoes: Acoes }) {
  const [aberto, setAberto] = useState(false);
  const caixa = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!aberto) return;
    const fora = (e: MouseEvent) => !caixa.current?.contains(e.target as Node) && setAberto(false);
    window.addEventListener('mousedown', fora);
    return () => window.removeEventListener('mousedown', fora);
  }, [aberto]);

  const itens: Array<{ rotulo: string; Icone: typeof FiFolder; fazer: () => void; perigo?: boolean }> = [];
  if (jogo.tipo === 'download' && jogo.disponivel && !ocupado) {
    if (estado?.instalado) {
      itens.push({ rotulo: 'Abrir pasta', Icone: FiFolder, fazer: () => acoes.abrirPasta(jogo.slug) });
      itens.push({ rotulo: 'Criar atalhos (área de trabalho e Iniciar)', Icone: FiLink, fazer: () => acoes.criarAtalhos(jogo.slug) });
      if (estado.origem === 'localizado')
        itens.push({ rotulo: 'Esquecer este local', Icone: FiX, fazer: () => acoes.esquecer(jogo.slug) });
      if (estado.desinstalavel)
        itens.push({ rotulo: 'Desinstalar', Icone: FiTrash2, fazer: () => acoes.desinstalar(jogo.slug), perigo: true });
    } else if (estado?.incompleto) {
      itens.push({ rotulo: 'Descartar o que já baixou', Icone: FiTrash2, fazer: () => acoes.desinstalar(jogo.slug), perigo: true });
    } else {
      itens.push({ rotulo: 'Já tenho instalado — localizar', Icone: FiSearch, fazer: () => acoes.localizar(jogo.slug) });
    }
  }
  if (jogo.tipo === 'download' && jogo.disponivel && jogo.downloadUrl)
    itens.push({ rotulo: 'Baixar pelo navegador', Icone: FiGlobe, fazer: () => acoes.abrirLink(jogo.downloadUrl!) });
  if (jogo.siteUrl) itens.push({ rotulo: 'Site oficial', Icone: FiExternalLink, fazer: () => acoes.abrirLink(jogo.siteUrl!) });

  if (itens.length === 0) return null;
  return (
    <div ref={caixa} className="relative">
      <button
        type="button"
        className="btn-secondary px-3 py-3"
        onClick={() => setAberto((a) => !a)}
        aria-label="Mais opções"
        aria-expanded={aberto}
      >
        <FiMoreHorizontal aria-hidden="true" />
      </button>
      {aberto && (
        <div className="absolute left-0 top-full z-20 mt-2 w-72 overflow-hidden rounded-lg border border-surface-lighter bg-surface shadow-2xl">
          {itens.map(({ rotulo, Icone, fazer, perigo }) => (
            <button
              key={rotulo}
              type="button"
              onClick={() => {
                setAberto(false);
                fazer();
              }}
              className={`flex w-full items-center gap-3 px-4 py-2.5 text-left text-sm transition hover:bg-surface-light ${
                perigo ? 'text-red-400' : 'text-slate-300 hover:text-white'
              }`}
            >
              <Icone className="shrink-0" aria-hidden="true" /> {rotulo}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function AvisoFalha({ jogo, falha, acoes }: { jogo: Jogo; falha: Falha; acoes: Acoes }) {
  const pausado = falha.codigo === 'cancelado';
  return (
    <div
      className={`mx-10 mt-6 flex items-start gap-3 rounded-lg border p-4 text-sm ${
        pausado ? 'border-surface-lighter bg-surface-light text-slate-300' : 'border-amber-500/40 bg-amber-500/10 text-amber-200'
      }`}
    >
      <FiAlertTriangle className="mt-0.5 shrink-0" aria-hidden="true" />
      <div className="flex-1" data-texto>
        <p>{falha.mensagem}</p>
        {falha.codigo === 'desafio' && jogo.downloadUrl && (
          <p className="mt-1 text-amber-200/80">
            Dá para baixar pelo navegador e rodar o instalador; o launcher encontra o jogo sozinho depois.
          </p>
        )}
      </div>
      {falha.codigo === 'atualizacao' && (
        <button type="button" className="btn-secondary shrink-0 px-3 py-1.5 text-xs" onClick={() => acoes.jogar(jogo.slug, true)}>
          <FiPlay aria-hidden="true" /> {jogo.categoria === 'projeto' ? 'Abrir' : 'Jogar'} mesmo assim
        </button>
      )}
      {(falha.codigo === 'desafio' || falha.codigo === 'rede') && jogo.downloadUrl && (
        <button type="button" className="btn-secondary shrink-0 px-3 py-1.5 text-xs" onClick={() => acoes.abrirLink(jogo.downloadUrl!)}>
          <FiGlobe aria-hidden="true" /> Baixar pelo navegador
        </button>
      )}
      <button type="button" onClick={() => acoes.limparFalha(jogo.slug)} aria-label="Dispensar" className="shrink-0 opacity-70 hover:opacity-100">
        <FiX />
      </button>
    </div>
  );
}

const ORIGEM: Record<NonNullable<EstadoJogo['origem']>, string> = {
  registro: 'instalador',
  pasta: 'Firawynix Center',
  caminho: 'pasta padrão',
  localizado: 'você apontou',
};

function rotuloTipo(jogo: Jogo): string {
  const projeto = jogo.categoria === 'projeto';
  if (jogo.tipo === 'web') return projeto ? 'Site (abre numa janela do launcher)' : 'Jogo no navegador';
  if (!jogo.disponivel) return projeto ? 'Programa disponível somente para Windows' : 'Jogo disponível somente para Windows';
  return projeto ? 'Programa para este sistema' : 'Instalável';
}

export default function JogoView({
  jogo,
  estado,
  progresso,
  acao,
  falha,
  acoes,
}: {
  jogo: Jogo;
  estado?: EstadoJogo;
  progresso?: Progresso;
  acao?: Acao;
  falha?: Falha;
  acoes: Acoes;
}) {
  const peso = formatarTamanho(jogo.downloadTamanho);
  const linha = (rotulo: string, valor: React.ReactNode) => (
    <div className="flex justify-between gap-4 py-1.5">
      <dt className="shrink-0 text-slate-500">{rotulo}</dt>
      <dd className="min-w-0 truncate text-right text-slate-200" data-texto>
        {valor}
      </dd>
    </div>
  );

  return (
    <div className="h-full overflow-y-auto">
      <div className="relative h-[340px] overflow-hidden">
        <GameArt jogo={jogo} variante="banner" />
        <div className="absolute inset-0 bg-gradient-to-t from-surface via-surface/40 to-transparent" />
        <div className="absolute inset-0 bg-gradient-to-r from-surface/80 via-transparent to-transparent" />
        <div className="absolute bottom-8 left-10 right-10">
          <div className="mb-3 flex flex-wrap gap-2">
            {jogo.generos.map((g) => (
              <span key={g} className="rounded-full bg-surface/75 px-2.5 py-1 text-[11px] text-slate-300">
                {g}
              </span>
            ))}
          </div>
          <h1 className="text-5xl font-black tracking-tight text-white">{jogo.nome}</h1>
          {jogo.subtitulo && <p className="mt-2 max-w-2xl text-lg text-slate-300">{jogo.subtitulo}</p>}
        </div>
      </div>

      <div className="sticky top-0 z-10 flex flex-wrap items-center gap-3 border-b border-surface-lighter bg-surface/90 px-10 py-4 backdrop-blur">
        <BotaoPrincipal jogo={jogo} estado={estado} progresso={progresso} acao={acao} acoes={acoes} />
        <Menu jogo={jogo} estado={estado} ocupado={!!acao} acoes={acoes} />
        <div className="ml-auto flex gap-6 text-xs text-slate-500">
          {jogo.tipo === 'download' && estado?.instalado && estado.versaoInstalada && (
            <span>
              Instalado <span className="font-mono text-slate-300">v{estado.versaoInstalada.replace(/^v/, '')}</span>
            </span>
          )}
          {jogo.tipo === 'download' && jogo.versao && (
            <span>
              Publicado <span className="font-mono text-slate-300">v{jogo.versao.replace(/^v/, '')}</span>
            </span>
          )}
        </div>
      </div>

      {falha && <AvisoFalha jogo={jogo} falha={falha} acoes={acoes} />}

      <div className="grid grid-cols-[1fr_300px] gap-8 px-10 py-8">
        <div className="space-y-6">
          {jogo.descricao && (
            <div className="card whitespace-pre-line leading-relaxed text-slate-300" data-texto>
              {jogo.descricao}
            </div>
          )}
          {jogo.extras.length > 0 && (
            <div>
              <p className="section-title">$ outros jeitos de {jogo.categoria === 'projeto' ? 'usar' : 'jogar'}</p>
              <div className="flex flex-wrap gap-2">
                {jogo.extras.map((e) => {
                  const microsoftStore = e.url.startsWith('https://apps.microsoft.com/');
                  return (
                    <button
                      key={e.url}
                      type="button"
                      className={`btn-secondary px-3 py-2 text-sm ${microsoftStore ? 'border-slate-500 bg-black text-white hover:bg-slate-900' : ''}`}
                      onClick={() => acoes.abrirLink(e.url)}
                    >
                      {microsoftStore && <FaMicrosoft size={14} aria-hidden="true" />}
                      {e.rotulo} <FiExternalLink size={13} aria-hidden="true" />
                    </button>
                  );
                })}
              </div>
            </div>
          )}
        </div>

        <aside className="card h-fit p-5 text-sm">
          <p className="section-title">$ detalhes</p>
          <dl className="divide-y divide-surface-lighter/60">
            {linha('Tipo', rotuloTipo(jogo))}
            {jogo.tipo === 'download' &&
              (jogo.winInstalador === 'manifesto'
                ? linha('Download', 'direto do servidor do jogo')
                : peso && linha('Instalador', <span className="font-mono">{peso}</span>))}
            {jogo.tipo === 'download' &&
              formatarTamanho(jogo.instaladoTamanho) &&
              linha('Espaço em disco', <span className="font-mono">{formatarTamanho(jogo.instaladoTamanho)}</span>)}
            {jogo.tipo === 'download' &&
              linha(
                'Situação',
                estado?.instalado ? (
                  <span className="text-emerald-400">Instalado</span>
                ) : estado?.incompleto ? (
                  <span className="text-amber-400">Instalação incompleta</span>
                ) : (
                  <span className="text-slate-400">Não instalado</span>
                ),
              )}
            {jogo.tipo === 'download' &&
              linha('Atualização', jogo.winInstalador === 'manual' ? 'pelo próprio programa' : `conferida a cada ${verbo(jogo)}`)}
            {estado?.pasta && linha('Pasta', <span title={estado.pasta}>{estado.pasta}</span>)}
            {estado?.origem && linha('Encontrado por', ORIGEM[estado.origem])}
            {jogo.tipo === 'web' && jogo.jogarUrl && linha('Endereço', new URL(jogo.jogarUrl).hostname)}
          </dl>
          {jogo.siteUrl && (
            <button type="button" className="btn-secondary mt-4 w-full py-2 text-xs" onClick={() => acoes.abrirLink(jogo.siteUrl!)}>
              Site oficial <FiExternalLink aria-hidden="true" />
            </button>
          )}
        </aside>
      </div>
    </div>
  );
}
