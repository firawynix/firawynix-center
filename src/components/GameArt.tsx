import { useState } from 'react';
import type { Jogo } from '../api';

type Variante = 'capa' | 'banner' | 'icone';

function iniciais(nome: string): string {
  const partes = nome.trim().split(/\s+/).filter(Boolean);
  if (partes.length === 1) return partes[0].slice(0, 2).toUpperCase();
  return (partes[0][0] + partes[partes.length - 1][0]).toUpperCase();
}

/**
 * Arte do jogo com a mesma capa provisória do site: sem imagem (ou se ela não
 * carregar — por exemplo, a Cloudflare barrando), desenha com a cor do jogo.
 */
export default function GameArt({
  jogo,
  variante,
  className = '',
}: {
  jogo: Pick<Jogo, 'nome' | 'cor' | 'capa' | 'banner' | 'icone'>;
  variante: Variante;
  className?: string;
}) {
  const src =
    variante === 'capa' ? jogo.capa ?? jogo.banner : variante === 'banner' ? jogo.banner ?? jogo.capa : jogo.icone;
  const [falhou, setFalhou] = useState(false);
  const valido = src && src.startsWith('https://');

  if (valido && !falhou) {
    return (
      <img
        src={src}
        alt=""
        draggable={false}
        onError={() => setFalhou(true)}
        className={`h-full w-full object-cover ${className}`}
      />
    );
  }

  const cor = /^#[0-9a-fA-F]{6}$/.test(jogo.cor) ? jogo.cor : '#22d3ee';
  return (
    <div
      aria-hidden="true"
      className={`relative flex h-full w-full items-center justify-center overflow-hidden ${className}`}
      style={{
        background: `radial-gradient(120% 90% at 20% 10%, ${cor}55 0%, transparent 55%), radial-gradient(90% 80% at 90% 100%, ${cor}33 0%, transparent 60%), linear-gradient(160deg, #1e293b 0%, #0f172a 70%)`,
      }}
    >
      <div
        className="absolute inset-0 opacity-[0.12]"
        style={{
          backgroundImage: `linear-gradient(${cor} 1px, transparent 1px), linear-gradient(90deg, ${cor} 1px, transparent 1px)`,
          backgroundSize: variante === 'icone' ? '6px 6px' : '28px 28px',
        }}
      />
      {variante === 'icone' ? (
        <span className="relative font-mono text-[11px] font-bold" style={{ color: cor }}>
          {iniciais(jogo.nome)}
        </span>
      ) : variante === 'banner' ? (
        // o título do jogo fica à esquerda: as iniciais vão para a direita
        <span
          className="absolute right-[7%] top-1/2 -translate-y-1/2 font-mono text-[200px] font-black leading-none tracking-tight opacity-50"
          style={{ color: cor, textShadow: `0 0 60px ${cor}55` }}
        >
          {iniciais(jogo.nome)}
        </span>
      ) : (
        <div className="relative px-4 text-center">
          <span className="block font-mono text-5xl font-black tracking-tight" style={{ color: cor, textShadow: `0 0 32px ${cor}66` }}>
            {iniciais(jogo.nome)}
          </span>
          <span className="mt-3 block text-xs font-semibold uppercase tracking-[0.2em] text-slate-300">{jogo.nome}</span>
        </div>
      )}
    </div>
  );
}
