import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';

// Menu de contexto do navegador ("Recarregar", "Inspecionar") não faz sentido
// num app; fica só onde a pessoa pode querer copiar texto.
document.addEventListener('contextmenu', (e) => {
  const alvo = e.target as HTMLElement | null;
  if (!alvo?.closest('input, textarea, [data-texto]')) e.preventDefault();
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
