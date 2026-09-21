import { FiCompass, FiExternalLink, FiSearch } from 'react-icons/fi';
import { useMemo, useState } from 'react';
import { Aba, Acao, daAba, EstadoJogo, Jogo, Progresso } from '../api';
import GameArt from './GameArt';

const ABAS: Array<{ id: Aba; rotulo: string }> = [
  { id: 'jogos', rotulo: 'Jogos' },
  { id: 'projetos', rotulo: 'Projetos' },
  { id: 'todos', rotulo: 'Todos' },
];

function Status({ jogo, estado, progresso, acao }: { jogo: Jogo; estado?: EstadoJogo; progresso?: Progresso; acao?: Acao }) {
  if (acao === 'desinstalar') return <span className="block text-[11px] text-brand">Desinstalando...</span>;
  const comBarra =
    progresso?.total &&
    (progresso.fase === 'baixando' || (progresso.fase === 'instalando' && progresso.silencioso));
  if (acao && progresso && comBarra) {
    const pct = Math.min(99, Math.floor((progresso.baixado / progresso.total!) * 100));
    const rotulo = acao === 'jogar' ? 'Atualizando' : progresso.fase === 'baixando' ? 'Baixando' : 'Instalando';
    return (
      <span className="mt-1 block">
        <span className="block text-[11px] text-brand">
          {rotulo} {pct}%
        </span>
        <span className="mt-1 block h-1 overflow-hidden rounded-full bg-surface-lighter">
          <span className="block h-full rounded-full bg-brand" style={{ width: `${pct}%` }} />
        </span>
      </span>
    );
  }
  if (acao === 'jogar') return <span className="block text-[11px] text-brand">Abrindo...</span>;
  if (acao) return <span className="block text-[11px] text-brand">Instalando...</span>;
  if (jogo.tipo === 'web')
    return <span className="block text-[11px] text-slate-500">{jogo.categoria === 'projeto' ? 'Site' : 'No navegador'}</span>;
  if (estado?.incompleto)
    return (
      <span className="flex items-center gap-1.5 text-[11px] text-amber-400">
        <i className="h-1.5 w-1.5 rounded-full bg-amber-400" /> Incompleto
      </span>
    );
  if (estado?.instalado)
    return (
      <span className="flex items-center gap-1.5 text-[11px] text-emerald-400">
        <i className="h-1.5 w-1.5 rounded-full bg-emerald-400" /> Instalado
      </span>
    );
  return <span className="block text-[11px] text-slate-500">Não instalado</span>;
}

export default function Sidebar({
  itens,
  aba,
  onAba,
  estados,
  progresso,
  ocupados,
  vista,
  onVista,
  versaoApp,
}: {
  itens: Jogo[];
  aba: Aba;
  onAba: (a: Aba) => void;
  estados: Record<string, EstadoJogo>;
  progresso: Record<string, Progresso>;
  ocupados: Record<string, Acao | undefined>;
  vista: string;
  onVista: (v: string) => void;
  versaoApp: string;
}) {
  const [soInstalados, setSoInstalados] = useState(false);
  const [busca, setBusca] = useState('');

  const daVez = useMemo(() => itens.filter((j) => daAba(j, aba)), [itens, aba]);
  const lista = useMemo(() => {
    const termo = busca.trim().toLowerCase();
    return daVez
      .filter((j) => !soInstalados || j.tipo === 'web' || estados[j.slug]?.instalado)
      .filter((j) => !termo || j.nome.toLowerCase().includes(termo))
      .sort((a, b) => a.nome.localeCompare(b.nome, 'pt-BR'));
  }, [daVez, estados, soInstalados, busca]);

  const item = (ativo: boolean) =>
    `flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition ${
      ativo ? 'bg-brand/10 text-brand' : 'text-slate-300 hover:bg-surface-light hover:text-white'
    }`;

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-surface-lighter bg-surface-light/40">
      <nav className="space-y-3 p-3">
        <button type="button" className={item(vista === 'descobrir')} onClick={() => onVista('descobrir')}>
          <FiCompass aria-hidden="true" /> <span className="font-semibold">Descobrir</span>
        </button>
        <div role="tablist" aria-label="O que mostrar" className="grid grid-cols-3 gap-1 rounded-lg bg-surface p-1 text-xs font-semibold">
          {ABAS.map((a) => (
            <button
              key={a.id}
              type="button"
              role="tab"
              aria-selected={aba === a.id}
              onClick={() => onAba(a.id)}
              className={`rounded-md py-1.5 transition ${
                aba === a.id ? 'bg-brand/15 text-brand' : 'text-slate-400 hover:bg-surface-light hover:text-white'
              }`}
            >
              {a.rotulo}
            </button>
          ))}
        </div>
      </nav>

      <div className="flex items-center justify-between px-5 pb-2">
        <span className="whitespace-nowrap font-mono text-[10px] uppercase tracking-[0.15em] text-slate-500">
          Biblioteca · {daVez.length}
        </span>
        <button
          type="button"
          aria-pressed={soInstalados}
          onClick={() => setSoInstalados((s) => !s)}
          className={`rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wider ${
            soInstalados ? 'bg-surface-lighter text-white' : 'text-slate-500 hover:text-slate-300'
          }`}
        >
          Instalados
        </button>
      </div>
      <div className="px-3 pb-2">
        <label className="flex items-center gap-2 rounded-lg border border-surface-lighter bg-surface px-3 py-1.5 text-sm focus-within:border-brand">
          <FiSearch className="shrink-0 text-slate-500" aria-hidden="true" />
          <input
            value={busca}
            onChange={(e) => setBusca(e.target.value)}
            placeholder="Buscar"
            className="w-full bg-transparent text-slate-200 placeholder-slate-500 outline-none"
          />
        </label>
      </div>

      <div className="flex-1 space-y-0.5 overflow-y-auto px-3 pb-3">
        {lista.map((jogo) => (
          <button key={jogo.id} type="button" className={item(vista === jogo.slug)} onClick={() => onVista(jogo.slug)}>
            <span className="h-9 w-9 shrink-0 overflow-hidden rounded-md border border-surface-lighter">
              <GameArt jogo={jogo} variante="icone" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-medium">{jogo.nome}</span>
              <Status jogo={jogo} estado={estados[jogo.slug]} progresso={progresso[jogo.slug]} acao={ocupados[jogo.slug]} />
            </span>
          </button>
        ))}
        {lista.length === 0 && <p className="px-3 py-4 text-xs text-slate-500">Nada por aqui.</p>}
      </div>

      <div className="flex items-center justify-between border-t border-surface-lighter p-3 text-xs text-slate-500">
        <span className="font-mono">v{versaoApp}</span>
        <a
          href="#"
          onClick={(e) => {
            e.preventDefault();
            onVista('__site');
          }}
          className="inline-flex items-center gap-1 transition hover:text-brand"
        >
          Central no navegador <FiExternalLink size={11} aria-hidden="true" />
        </a>
      </div>
    </aside>
  );
}
