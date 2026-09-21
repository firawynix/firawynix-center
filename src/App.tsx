import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FiAlertTriangle, FiDownloadCloud, FiRefreshCw } from 'react-icons/fi';
import { Aba, Acao, Catalogo, cmd, daAba, EstadoJogo, Falha, falhaDe, Jogo, NovaVersao, Progresso } from './api';
import Descobrir from './components/Descobrir';
import JogoView, { Acoes } from './components/JogoView';
import { ModalDesinstalar, ModalInstalar } from './components/Modais';
import Sidebar from './components/Sidebar';
import TitleBar from './components/TitleBar';

/** Relê o catálogo (e procura versão nova do launcher) de tempos em tempos. */
const RECARREGAR_MS = 10 * 60 * 1000;
/**
 * A mesma versão já foi tentada há menos que isso e o launcher continua velho:
 * não tenta sozinho de novo (instalador com defeito viraria um laço de reinício).
 */
const JANELA_TENTATIVA_MS = 30 * 60 * 1000;

/* localStorage pode faltar (bloqueado, limpo): tudo aqui tem plano B */
function lerAba(): Aba {
  try {
    const v = localStorage.getItem('fw-aba');
    if (v === 'jogos' || v === 'projetos' || v === 'todos') return v;
  } catch {
    /* sem armazenamento: começa em Jogos */
  }
  return 'jogos';
}

function gravarAba(a: Aba) {
  try {
    localStorage.setItem('fw-aba', a);
  } catch {
    /* só conveniência */
  }
}

function jaTentou(versao: string): boolean {
  try {
    const t = JSON.parse(localStorage.getItem('fw-atualizacao') ?? 'null') as { versao: string; quando: number } | null;
    return t?.versao === versao && Date.now() - t.quando < JANELA_TENTATIVA_MS;
  } catch {
    return false;
  }
}

function marcarTentativa(versao: string) {
  try {
    localStorage.setItem('fw-atualizacao', JSON.stringify({ versao, quando: Date.now() }));
  } catch {
    /* sem memória da tentativa: o limite do Rust (nada ocupado) continua valendo */
  }
}

export default function App() {
  const [catalogo, setCatalogo] = useState<Catalogo | null>(null);
  const [erroFatal, setErroFatal] = useState<string | null>(null);
  const [estados, setEstados] = useState<Record<string, EstadoJogo>>({});
  const [progresso, setProgresso] = useState<Record<string, Progresso>>({});
  const [ocupados, setOcupados] = useState<Record<string, Acao | undefined>>({});
  const [falhas, setFalhas] = useState<Record<string, Falha>>({});
  const [vista, setVista] = useState('descobrir');
  const [aba, setAba] = useState<Aba>(lerAba);
  const [aviso, setAviso] = useState<string | null>(null);
  const [modalInstalar, setModalInstalar] = useState<Jogo | null>(null);
  const [modalDesinstalar, setModalDesinstalar] = useState<Jogo | null>(null);
  const [novaVersao, setNovaVersao] = useState<NovaVersao | null>(null);
  const [atualizandoLauncher, setAtualizandoLauncher] = useState(false);
  const [updaterFalhou, setUpdaterFalhou] = useState(false);
  const avisoTimer = useRef<number>();

  const mostrar = useCallback((texto: string, ms = 4500) => {
    setAviso(texto);
    window.clearTimeout(avisoTimer.current);
    avisoTimer.current = window.setTimeout(() => setAviso(null), ms);
  }, []);

  const recarregarEstados = useCallback(async () => {
    try {
      const lista = await cmd.estados();
      setEstados(Object.fromEntries(lista.map((e) => [e.slug, e])));
    } catch {
      /* detecção falhou: fica o que já havia */
    }
  }, []);

  const carregar = useCallback(async () => {
    try {
      setCatalogo(await cmd.catalogo());
      setErroFatal(null);
      await recarregarEstados();
    } catch (e) {
      setErroFatal(falhaDe(e).mensagem);
    }
  }, [recarregarEstados]);

  useEffect(() => {
    void carregar();
    const t = window.setInterval(() => void carregar(), RECARREGAR_MS);
    return () => window.clearInterval(t);
  }, [carregar]);

  // voltou para a janela (instalou algo por fora, desinstalou pelo Windows): redetecta
  useEffect(() => {
    const f = () => void recarregarEstados();
    window.addEventListener('focus', f);
    return () => window.removeEventListener('focus', f);
  }, [recarregarEstados]);

  useEffect(() => {
    const parar = listen<Progresso>('progresso', (ev) =>
      setProgresso((p) => ({ ...p, [ev.payload.slug]: ev.payload })),
    );
    return () => {
      void parar.then((f) => f());
    };
  }, []);

  /* ---------- o launcher se atualiza sozinho ---------- */

  const procurar = useCallback(() => {
    cmd
      .procurarAtualizacao()
      .then((v) => {
        setNovaVersao(v);
        setUpdaterFalhou(false);
      })
      .catch(() => setUpdaterFalhou(true));
  }, []);

  useEffect(() => {
    procurar();
    const t = window.setInterval(procurar, RECARREGAR_MS);
    return () => window.clearInterval(t);
  }, [procurar]);

  const algumOcupado = Object.values(ocupados).some(Boolean);

  const aplicar = useCallback(
    (v: NovaVersao) => {
      marcarTentativa(v.versao);
      setAtualizandoLauncher(true);
      // deu certo = este launcher fecha e a versão nova abre sozinha
      cmd.aplicarAtualizacao().catch((e) => {
        setAtualizandoLauncher(false);
        mostrar(falhaDe(e).mensagem, 8000);
      });
    },
    [mostrar],
  );

  // nada instalando/baixando: atualiza sem perguntar; ocupado, espera terminar
  useEffect(() => {
    if (novaVersao && !atualizandoLauncher && !algumOcupado && !jaTentou(novaVersao.versao)) aplicar(novaVersao);
  }, [novaVersao, atualizandoLauncher, algumOcupado, aplicar]);

  /* ---------- ações dos jogos ---------- */

  const itens = useMemo(() => (catalogo ? [...catalogo.jogos, ...catalogo.projetos] : []), [catalogo]);
  const achar = (slug: string) => itens.find((j) => j.slug === slug);
  const nome = (slug: string) => achar(slug)?.nome ?? slug;

  const falhar = (slug: string, e: unknown) => setFalhas((f) => ({ ...f, [slug]: falhaDe(e) }));
  const limparFalha = (slug: string) =>
    setFalhas((f) => {
      const { [slug]: _, ...resto } = f;
      return resto;
    });
  const ocupar = (slug: string, acao?: Acao) => setOcupados((o) => ({ ...o, [slug]: acao }));
  const soltar = (slug: string) => {
    ocupar(slug, undefined);
    setProgresso(({ [slug]: _, ...resto }) => resto);
  };

  function confirmarInstalar(slug: string, area: boolean, menu: boolean) {
    limparFalha(slug);
    ocupar(slug, 'instalar');
    cmd
      .instalar(slug, area, menu)
      .then((e) => {
        setEstados((s) => ({ ...s, [slug]: e }));
        mostrar(e.instalado ? `${nome(slug)} instalado.` : `Instalador de ${nome(slug)} concluído.`);
      })
      .catch((e) => falhar(slug, e))
      .finally(() => soltar(slug));
  }

  function confirmarDesinstalar(slug: string, completo: boolean) {
    limparFalha(slug);
    ocupar(slug, 'desinstalar');
    cmd
      .desinstalar(slug, completo)
      .then((r) => {
        setEstados((s) => ({ ...s, [slug]: r.estado }));
        if (r.restou.length) mostrar(`${nome(slug)} desinstalado. Não saiu: ${r.restou.join(' · ')}`, 12000);
        else mostrar(`${nome(slug)} desinstalado${completo ? ' sem deixar rastro' : ''}.`);
      })
      .catch((e) => falhar(slug, e))
      .finally(() => soltar(slug));
  }

  const acoes: Acoes = {
    instalar: (slug) => {
      const j = achar(slug);
      if (j) setModalInstalar(j);
    },
    cancelar: (slug) => void cmd.cancelar(slug),
    jogar: (slug, pular = false) => {
      limparFalha(slug);
      ocupar(slug, 'jogar');
      cmd
        .jogar(slug, pular)
        .then((r) => {
          if (r.aviso) mostrar(r.aviso, 7000);
          else mostrar(r.atualizado ? `${nome(slug)} atualizado. Abrindo...` : `Abrindo ${nome(slug)}...`);
        })
        .catch((e) => falhar(slug, e))
        .finally(() => {
          soltar(slug);
          void recarregarEstados();
        });
    },
    jogarWeb: (slug) => {
      limparFalha(slug);
      cmd.jogarWeb(slug).catch((e) => falhar(slug, e));
    },
    desinstalar: (slug) => {
      const j = achar(slug);
      if (j) setModalDesinstalar(j);
    },
    criarAtalhos: (slug) => {
      cmd
        .criarAtalhos(slug, true, true)
        .then(() => mostrar('Atalhos na área de trabalho e no menu Iniciar prontos.'))
        .catch((e) => falhar(slug, e));
    },
    localizar: (slug) => {
      cmd
        .localizar(slug)
        .then((e) => {
          if (!e) return;
          setEstados((s) => ({ ...s, [slug]: e }));
          mostrar(e.instalado ? `${nome(slug)} encontrado.` : 'Esse arquivo não serve.');
        })
        .catch((e) => falhar(slug, e));
    },
    esquecer: (slug) => {
      cmd
        .esquecer(slug)
        .then((e) => setEstados((s) => ({ ...s, [slug]: e })))
        .catch((e) => falhar(slug, e));
    },
    abrirPasta: (slug) => void cmd.abrirPasta(slug).catch((e) => falhar(slug, e)),
    abrirLink: (url) => void cmd.abrirLink(url).catch((e) => mostrar(falhaDe(e).mensagem)),
    limparFalha,
  };

  /** Plano B: feed do updater fora do ar/sem assinatura — baixa o instalador pelo catálogo. */
  function atualizarPeloCatalogo() {
    setAtualizandoLauncher(true);
    mostrar('Baixando a versão nova do launcher...');
    cmd
      .atualizarLauncher()
      .catch((e) => mostrar(falhaDe(e).mensagem))
      .finally(() => setAtualizandoLauncher(false));
  }

  function mudarVista(v: string) {
    if (v === '__site') {
      if (catalogo) acoes.abrirLink(catalogo.base);
      return;
    }
    setVista(v);
  }

  function mudarAba(a: Aba) {
    setAba(a);
    gravarAba(a);
    // a lista mudou: o Descobrir mostra a aba nova
    setVista('descobrir');
  }

  const jogo = itens.find((j) => j.slug === vista);
  const pLauncher = progresso.launcher;
  const pctLauncher = pLauncher?.total ? ` · ${Math.floor((pLauncher.baixado / pLauncher.total) * 100)}%` : '';
  const planoB = !novaVersao && updaterFalhou && catalogo?.atualizacao && catalogo.launcher;

  return (
    <div className="flex h-full flex-col">
      {/* a página de apoio é a mesma do portfólio; ?de=center diz de onde a pessoa veio */}
      <TitleBar onApoiar={catalogo ? () => acoes.abrirLink(`${catalogo.base}/apoie?de=center`) : undefined} />

      {!catalogo ? (
        <div className="flex flex-1 items-center justify-center">
          {erroFatal ? (
            <div className="text-center">
              <p className="mb-4 text-slate-400">{erroFatal}</p>
              <button type="button" className="btn-secondary" onClick={() => void carregar()}>
                <FiRefreshCw aria-hidden="true" /> Tentar de novo
              </button>
            </div>
          ) : (
            <div className="h-10 w-10 animate-spin rounded-full border-2 border-slate-600 border-t-brand" />
          )}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1">
          <Sidebar
            itens={itens}
            aba={aba}
            onAba={mudarAba}
            estados={estados}
            progresso={progresso}
            ocupados={ocupados}
            vista={vista}
            onVista={mudarVista}
            versaoApp={catalogo.versaoApp}
          />
          <main className="relative flex min-w-0 flex-1 flex-col">
            {catalogo.aviso && (
              <div className="flex items-center gap-3 border-b border-amber-500/30 bg-amber-500/10 px-6 py-2 text-xs text-amber-200">
                <FiAlertTriangle className="shrink-0" aria-hidden="true" />
                <span className="flex-1" data-texto>
                  {catalogo.aviso}
                </span>
                <button type="button" onClick={() => void carregar()} className="inline-flex items-center gap-1 font-semibold hover:text-white">
                  <FiRefreshCw aria-hidden="true" /> Tentar de novo
                </button>
              </div>
            )}
            {(novaVersao || planoB) && (
              <div className="flex items-center gap-3 border-b border-brand/30 bg-brand/10 px-6 py-2 text-xs text-brand">
                <FiDownloadCloud className="shrink-0" aria-hidden="true" />
                <span className="flex-1">
                  {atualizandoLauncher
                    ? `Atualizando o Firawynix Center para a versão ${novaVersao?.versao ?? catalogo.launcher?.versao}${pctLauncher} — ele fecha e abre de novo sozinho.`
                    : novaVersao && algumOcupado
                      ? `Versão ${novaVersao.versao} do Firawynix Center pronta: instala sozinha quando terminar o que está em andamento.`
                      : `Versão ${novaVersao?.versao ?? catalogo.launcher?.versao} do Firawynix Center disponível${
                          catalogo.launcher?.notas ? ` — ${catalogo.launcher.notas}` : ''
                        }`}
                </span>
                {!atualizandoLauncher && !algumOcupado && (
                  <button
                    type="button"
                    onClick={() => (novaVersao ? aplicar(novaVersao) : atualizarPeloCatalogo())}
                    className="rounded-md bg-brand px-3 py-1 font-bold text-slate-900 hover:bg-brand-glow"
                  >
                    Atualizar agora
                  </button>
                )}
              </div>
            )}

            <div className="min-h-0 flex-1">
              {jogo ? (
                <JogoView
                  key={jogo.slug}
                  jogo={jogo}
                  estado={estados[jogo.slug]}
                  progresso={progresso[jogo.slug]}
                  acao={ocupados[jogo.slug]}
                  falha={falhas[jogo.slug]}
                  acoes={acoes}
                />
              ) : (
                <Descobrir itens={itens.filter((j) => daAba(j, aba))} aba={aba} estados={estados} onAbrir={setVista} />
              )}
            </div>

            {aviso && (
              <div className="pointer-events-none absolute bottom-5 right-5 max-w-md rounded-lg border border-surface-lighter bg-surface-light px-4 py-3 text-sm text-slate-200 shadow-2xl">
                {aviso}
              </div>
            )}
          </main>
        </div>
      )}

      {modalInstalar && (
        <ModalInstalar
          jogo={modalInstalar}
          onFechar={() => setModalInstalar(null)}
          onConfirmar={(area, menu) => {
            const slug = modalInstalar.slug;
            setModalInstalar(null);
            confirmarInstalar(slug, area, menu);
          }}
        />
      )}
      {modalDesinstalar && (
        <ModalDesinstalar
          jogo={modalDesinstalar}
          onFechar={() => setModalDesinstalar(null)}
          onConfirmar={(completo) => {
            const slug = modalDesinstalar.slug;
            setModalDesinstalar(null);
            confirmarDesinstalar(slug, completo);
          }}
        />
      )}
    </div>
  );
}
