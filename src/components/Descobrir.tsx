import { CSSProperties, useEffect, useMemo, useState } from 'react';
import { FiCheck, FiDownload, FiGlobe, FiPlay } from 'react-icons/fi';
import type { Aba, EstadoJogo, Jogo } from '../api';
import GameArt from './GameArt';

const TITULOS: Record<Aba, { prompt: string; titulo: string }> = {
  jogos: { prompt: '$ ls ./jogos', titulo: 'Todos os jogos' },
  projetos: { prompt: '$ ls ./projetos', titulo: 'Todos os projetos' },
  todos: { prompt: '$ ls ./tudo', titulo: 'Jogos e projetos' },
};

const pilula = 'inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider';

function Etiqueta({ jogo, instalado }: { jogo: Jogo; instalado: boolean }) {
  if (jogo.tipo === 'web')
    return (
      <span className={`${pilula} bg-surface/85 text-slate-300`}>
        <FiGlobe size={11} aria-hidden="true" /> {jogo.categoria === 'projeto' ? 'Site' : 'No navegador'}
      </span>
    );
  if (!jogo.disponivel)
    return <span className={`${pilula} bg-surface/85 text-slate-400`}>Somente Windows</span>;
  if (instalado)
    return (
      <span className={`${pilula} bg-emerald-500/90 font-bold text-slate-900`}>
        <FiCheck size={11} aria-hidden="true" /> Instalado
      </span>
    );
  return (
    <span className={`${pilula} bg-surface/85 text-slate-300`}>
      <FiDownload size={11} aria-hidden="true" /> Para baixar
    </span>
  );
}

function rotuloDoBotao(jogo: Jogo, instalado: boolean): string {
  if (!jogo.disponivel) return 'Ver detalhes';
  if (jogo.tipo === 'web' || instalado) return jogo.categoria === 'projeto' ? 'Abrir' : 'Jogar';
  return 'Ver e instalar';
}

export default function Descobrir({
  itens,
  aba,
  estados,
  onAbrir,
}: {
  itens: Jogo[];
  aba: Aba;
  estados: Record<string, EstadoJogo>;
  onAbrir: (slug: string) => void;
}) {
  const destaques = useMemo(() => {
    const d = itens.filter((j) => j.destaque);
    return (d.length ? d : itens).slice(0, 6);
  }, [itens]);
  const [atual, setAtual] = useState(0);

  // trocou de aba: o carrossel recomeça do primeiro
  useEffect(() => setAtual(0), [aba]);

  useEffect(() => {
    if (destaques.length < 2 || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const t = window.setInterval(() => setAtual((i) => (i + 1) % destaques.length), 7000);
    return () => window.clearInterval(t);
  }, [destaques.length]);

  const { prompt, titulo } = TITULOS[aba];

  return (
    <div className="h-full overflow-y-auto">
      {destaques.length > 0 && (
        <section className="relative h-[420px] overflow-hidden">
          {destaques.map((jogo, i) => {
            const instalado = !!estados[jogo.slug]?.instalado;
            return (
              <div
                key={jogo.id}
                className={`absolute inset-0 transition-opacity duration-700 ${i === atual ? 'opacity-100' : 'pointer-events-none opacity-0'}`}
              >
                <GameArt jogo={jogo} variante="banner" />
                <div className="absolute inset-0 bg-gradient-to-t from-surface via-surface/50 to-transparent" />
                <div className="absolute inset-0 bg-gradient-to-r from-surface/95 via-surface/30 to-transparent" />
                <div className="absolute bottom-12 left-10 right-10">
                  <p className="section-title">$ em destaque{aba === 'todos' ? ` · ${jogo.categoria === 'projeto' ? 'projeto' : 'jogo'}` : ''}</p>
                  <h1 className="max-w-2xl text-5xl font-black tracking-tight text-white">{jogo.nome}</h1>
                  {jogo.subtitulo && <p className="mt-3 max-w-xl text-lg text-slate-300">{jogo.subtitulo}</p>}
                  <button type="button" onClick={() => onAbrir(jogo.slug)} className="btn-primary mt-6 px-7 py-3">
                    {jogo.tipo === 'web' || instalado ? <FiPlay aria-hidden="true" /> : <FiDownload aria-hidden="true" />}
                    {rotuloDoBotao(jogo, instalado)}
                  </button>
                </div>
              </div>
            );
          })}
          {destaques.length > 1 && (
            <div className="absolute bottom-5 left-10 flex gap-2">
              {destaques.map((j, i) => (
                <button
                  key={j.id}
                  type="button"
                  aria-label={`Mostrar ${j.nome}`}
                  onClick={() => setAtual(i)}
                  className={`h-1.5 rounded-full transition-all ${i === atual ? 'w-10 bg-brand' : 'w-5 bg-slate-600 hover:bg-slate-400'}`}
                />
              ))}
            </div>
          )}
        </section>
      )}

      <section className="px-10 pb-12 pt-6">
        <p className="section-title">{prompt}</p>
        <h2 className="mb-6 text-2xl font-bold text-white">{titulo}</h2>
        {itens.length === 0 && <p className="text-slate-500">Nada publicado aqui ainda.</p>}
        <div className="grid grid-cols-[repeat(auto-fill,minmax(170px,1fr))] gap-x-5 gap-y-7">
          {itens.map((jogo) => (
            <button
              key={jogo.id}
              type="button"
              onClick={() => onAbrir(jogo.slug)}
              className="group text-left"
              style={{ '--cor': jogo.cor } as CSSProperties}
            >
              <div className="relative aspect-[3/4] overflow-hidden rounded-xl border border-surface-lighter bg-surface-light transition duration-300 group-hover:-translate-y-1 group-hover:border-[color:var(--cor)] group-hover:shadow-[0_22px_48px_-18px_var(--cor)]">
                <GameArt jogo={jogo} variante="capa" className="transition duration-500 group-hover:scale-105" />
                <div className="absolute left-2.5 top-2.5">
                  <Etiqueta jogo={jogo} instalado={!!estados[jogo.slug]?.instalado} />
                </div>
                {aba === 'todos' && (
                  <span className={`${pilula} absolute bottom-2.5 right-2.5 bg-surface/85 text-brand`}>
                    {jogo.categoria === 'projeto' ? 'Projeto' : 'Jogo'}
                  </span>
                )}
              </div>
              <p className="mt-2.5 truncate font-semibold text-white transition group-hover:text-brand">{jogo.nome}</p>
              {jogo.subtitulo && <p className="line-clamp-2 text-xs text-slate-400">{jogo.subtitulo}</p>}
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
