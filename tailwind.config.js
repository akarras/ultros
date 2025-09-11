/** @type {import('tailwindcss').Config} */
module.exports = {
  // Broadened template scanning so Tailwind finds classes across the monorepo
  content: {
    relative: true,
    files: [
      "./**/*.{html,rs,css,js,ts}",
      "./ultros-frontend/**/*.{rs,html,css,js,ts}",
      "./ultros-frontend/**/src/**/*.{rs,html}",
      "./style/**/*.{css}",
      "./ultros/static/**/*.{css,html,js}",
    ],
  },
  theme: {
    extend: {
      // Custom brand palette (violet-forward, tuned for a dark UI)
      colors: {
        // Override the default violet scale with a more muted brand spectrum
        violet: {
          50: "#f6f2ff",
          100: "#ede8ff",
          200: "#d8d0ff",
          300: "#beaefc",
          400: "#a38af7",
          500: "#8b67f0",
          600: "#744ee2",
          700: "#5e3ccd",
          800: "#4930a8",
          900: "#37267f",
          950: "#1a1538",
        },
        // Optional brand alias if you want to reference `brand-*` utilities
        brand: {
          50: "#f6f2ff",
          100: "#ede8ff",
          200: "#d8d0ff",
          300: "#beaefc",
          400: "#a38af7",
          500: "#8b67f0",
          600: "#744ee2",
          700: "#5e3ccd",
          800: "#4930a8",
          900: "#37267f",
          950: "#1a1538",
          DEFAULT: "#8b67f0",
        },
        // Optional background tokens for future use (dark-first)
        background: {
          DEFAULT: "#0a0a0b",
          muted: "#0d0c12",
          elevated: "#11101a",
          panel: "#0f0e16",
        },
      },
    },
  },
  plugins: [],
};
