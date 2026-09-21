/** @type {import('tailwindcss').Config} */
// Mesma paleta do portfólio (firawynix-portfolio/frontend/tailwind.config.js):
// ciano da marca + superfícies slate. Aqui a cor é fixa — o launcher não tem
// a persona H4K.
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        brand: {
          DEFAULT: '#22d3ee',
          dark: '#0e7490',
          glow: '#67e8f9',
        },
        surface: {
          DEFAULT: '#0f172a',
          light: '#1e293b',
          lighter: '#334155',
        },
      },
      fontFamily: {
        sans: ['Inter', 'Segoe UI', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'Cascadia Code', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
};
