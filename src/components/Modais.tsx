import { ReactNode, useEffect, useState } from 'react';
import { FiAlertTriangle, FiFolder, FiTrash2, FiX } from 'react-icons/fi';
import { cmd, falhaDe, formatarTamanho, Jogo, PlanoDesinstalar, PlanoInstalacao } from '../api';

/** Casca das duas janelas: fundo escuro, Esc fecha. */
function Moldura({ titulo, onFechar, children }: { titulo: string; onFechar: () => void; children: ReactNode }) {
  useEffect(() => {
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && onFechar();
    window.addEventListener('keydown', esc);
    return () => window.removeEventListener('keydown', esc);
  }, [onFechar]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6 backdrop-blur-sm" onMouseDown={onFechar}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={titulo}
        className="card w-full max-w-lg space-y-5 p-6 shadow-2xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4">
          <h2 className="text-xl font-bold text-white">{titulo}</h2>
          <button type="button" onClick={onFechar} aria-label="Fechar" className="text-slate-400 hover:text-white">
            <FiX size={20} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

function Opcao({ marcado, onChange, children }: { marcado: boolean; onChange: (v: boolean) => void; children: ReactNode }) {
  return (
    <label className="flex cursor-pointer items-start gap-3 rounded-lg border border-surface-lighter px-4 py-3 text-sm text-slate-200 transition hover:border-brand/60">
      <input type="checkbox" className="mt-0.5 h-4 w-4 accent-cyan-400" checked={marcado} onChange={(e) => onChange(e.target.checked)} />
      <span>{children}</span>
    </label>
  );
}

export function ModalInstalar({
  jogo,
  onFechar,
  onConfirmar,
}: {
  jogo: Jogo;
  onFechar: () => void;
  onConfirmar: (area: boolean, menu: boolean) => void;
}) {
  const [plano, setPlano] = useState<PlanoInstalacao | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [area, setArea] = useState(true);
  const [menu, setMenu] = useState(true);

  useEffect(() => {
    cmd
      .prepararInstalacao(jogo.slug)
      .then(setPlano)
      .catch((e) => setErro(falhaDe(e).mensagem));
  }, [jogo.slug]);

  function alterar() {
    cmd
      .escolherPasta(jogo.slug)
      .then((p) => p && setPlano(p))
      .catch((e) => setErro(falhaDe(e).mensagem));
  }

  const necessario = formatarTamanho(plano?.necessario);
  const livre = formatarTamanho(plano?.livre);
  const naoCabe = plano?.livre != null && plano?.necessario != null && plano.livre < plano.necessario;

  return (
    <Moldura titulo={`Instalar ${jogo.nome}`} onFechar={onFechar}>
      {!plano && !erro && <div className="barra-indeterminada h-1.5 rounded-full" />}
      {plano && (
        <>
          {plano.pasta && (
            <div>
              <p className="mb-1.5 text-xs font-semibold uppercase tracking-wider text-slate-500">Pasta</p>
              <div className="flex items-center gap-2">
                <code className="min-w-0 flex-1 truncate rounded-lg border border-surface-lighter bg-surface px-3 py-2 text-xs text-slate-200" title={plano.pasta}>
                  {plano.pasta}
                </code>
                {plano.podeEscolher && (
                  <button type="button" className="btn-secondary shrink-0 px-3 py-2 text-xs" onClick={alterar}>
                    <FiFolder aria-hidden="true" /> Alterar
                  </button>
                )}
              </div>
            </div>
          )}
          {(necessario || livre) && (
            <div className="flex justify-between gap-4 text-sm">
              {necessario && (
                <span className="text-slate-400">
                  Espaço necessário <b className="font-mono text-slate-100">{necessario}</b>
                </span>
              )}
              {livre && (
                <span className="text-slate-400">
                  Livre no disco <b className={`font-mono ${naoCabe ? 'text-red-400' : 'text-slate-100'}`}>{livre}</b>
                </span>
              )}
            </div>
          )}
          {plano.silencioso ? (
            <div className="space-y-2">
              <Opcao marcado={area} onChange={setArea}>
                Criar atalho na <b>área de trabalho</b>
              </Opcao>
              <Opcao marcado={menu} onChange={setMenu}>
                Criar atalho no <b>menu Iniciar</b>
              </Opcao>
              <p className="text-xs text-slate-500">
                O atalho abre o launcher do próprio jogo, que se atualiza sozinho — funciona até sem o Firawynix Center.
                Nenhuma janela de instalador abre: o progresso aparece aqui.
              </p>
            </div>
          ) : (
            <p className="text-sm text-slate-400">
              O instalador do {jogo.nome} abre a janela dele: pasta e atalhos se escolhem lá.
            </p>
          )}
          {naoCabe && (
            <p className="flex items-center gap-2 text-sm text-red-300">
              <FiAlertTriangle aria-hidden="true" /> Não cabe nesse disco — escolha outra pasta.
            </p>
          )}
        </>
      )}
      {erro && <p className="text-sm text-red-300">{erro}</p>}
      <div className="flex justify-end gap-3">
        <button type="button" className="btn-secondary" onClick={onFechar}>
          Cancelar
        </button>
        <button type="button" className="btn-primary" disabled={!plano || naoCabe} onClick={() => onConfirmar(area, menu)}>
          Instalar
        </button>
      </div>
    </Moldura>
  );
}

export function ModalDesinstalar({
  jogo,
  onFechar,
  onConfirmar,
}: {
  jogo: Jogo;
  onFechar: () => void;
  onConfirmar: (completo: boolean) => void;
}) {
  const [plano, setPlano] = useState<PlanoDesinstalar | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [completo, setCompleto] = useState(false);

  useEffect(() => {
    cmd
      .planoDesinstalar(jogo.slug)
      .then(setPlano)
      .catch((e) => setErro(falhaDe(e).mensagem));
  }, [jogo.slug]);

  return (
    <Moldura titulo={`Desinstalar ${jogo.nome}`} onFechar={onFechar}>
      {!plano && !erro && <div className="barra-indeterminada h-1.5 rounded-full" />}
      {plano && (
        <>
          <p className="text-sm text-slate-300">
            {plano.apagaPasta
              ? `Apaga os arquivos do jogo em ${plano.pasta}.`
              : plano.silencioso
                ? 'O desinstalador do jogo roda sem abrir janela.'
                : 'O desinstalador abre a janela dele para você concluir.'}{' '}
            {plano.precisaAdmin && 'Como foi instalado para todos os usuários, o Windows vai pedir permissão de administrador.'}
            {plano.atalhos.length > 0 && ' Os atalhos saem junto.'}
          </p>
          <Opcao marcado={completo} onChange={setCompleto}>
            <b>Remoção completa</b> — não deixa arquivo nem registro para trás
            {completo && (
              <ul className="mt-2 space-y-1 text-xs text-slate-400">
                {plano.completa.map((c) => (
                  <li key={c} className="flex gap-2 break-all">
                    <FiTrash2 className="mt-0.5 shrink-0 text-red-400" aria-hidden="true" /> {c}
                  </li>
                ))}
              </ul>
            )}
          </Opcao>
          {plano.avisos.map((a) => (
            <p key={a} className="flex items-start gap-2 text-xs text-amber-300">
              <FiAlertTriangle className="mt-0.5 shrink-0" aria-hidden="true" /> {a}
            </p>
          ))}
        </>
      )}
      {erro && <p className="text-sm text-red-300">{erro}</p>}
      <div className="flex justify-end gap-3">
        <button type="button" className="btn-secondary" onClick={onFechar}>
          Cancelar
        </button>
        <button
          type="button"
          disabled={!plano}
          onClick={() => onConfirmar(completo)}
          className="inline-flex items-center gap-2 rounded-lg bg-red-500 px-5 py-2.5 font-semibold text-white transition hover:bg-red-400 disabled:opacity-50"
        >
          <FiTrash2 aria-hidden="true" /> Desinstalar
        </button>
      </div>
    </Moldura>
  );
}
