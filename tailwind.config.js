/** @type {import('tailwindcss').Config} */
    module.exports = {
      content: {
        relative: true,
        files: ["*.html", "./**/*.rs"],
      },
      theme: {
        extend: {
          aria: {
            current: 'current'
          },
          backgroundSize: {
              'size-200': '200% 200%',
          },
          backgroundPosition: {
              'pos-0': '0% 0%',
              'pos-100': '100% 100%',
          },
        },
      },
      plugins: [],
    }
    