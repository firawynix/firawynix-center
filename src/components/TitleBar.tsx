import { getCurrentWindow } from '@tauri-apps/api/window';
import { FiMinus, FiSquare, FiTerminal, FiX } from 'react-icons/fi';

/**
 * Barra de título própria (a janela é sem moldura, como a do GOG Galaxy).
 * `data-tauri-drag-region` arrasta e o duplo clique maximiza; os botões ficam
 * fora da região para receberem o clique.
 */
export default function TitleBar({ onApoiar }: { onApoiar?: () => void }) {
  const janela = getCurrentWindow();
  const botao =
    'flex h-10 w-12 items-center justify-center text-slate-400 transition hover:bg-surface-lighter hover:text-white';
  return (
    <header data-tauri-drag-region className="flex h-10 shrink-0 items-center border-b border-surface-lighter bg-surface">
      <div data-tauri-drag-region className="flex flex-1 items-center gap-2 px-4 font-mono text-sm font-bold text-white">
        <FiTerminal className="pointer-events-none text-brand" aria-hidden="true" />
        <span className="pointer-events-none">
          Firawynix<span className="text-brand">Center</span>
        </span>
      </div>
      {onApoiar && (
        <button
          type="button"
          onClick={onApoiar}
          title="Apoie os projetos Firawynix (abre no navegador)"
          className="animate-apoiar mr-3 rounded-full bg-gradient-to-r from-brand via-brand-glow to-brand px-3 py-1 text-xs font-bold text-slate-900 transition hover:brightness-110"
        >
          ♥ Apoiar
        </button>
      )}
      <button type="button" className={botao} onClick={() => void janela.minimize()} aria-label="Minimizar">
        <FiMinus />
      </button>
      <button type="button" className={botao} onClick={() => void janela.toggleMaximize()} aria-label="Maximizar">
        <FiSquare size={13} />
      </button>
      <button
        type="button"
        className={`${botao} hover:!bg-red-500 hover:!text-white`}
        onClick={() => void janela.close()}
        aria-label="Fechar"
      >
        <FiX />
      </button>
    </header>
  );
}
