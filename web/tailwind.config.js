/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        bg: '#13111a',
        surface: '#1b1823',
        raised: '#242030',
        line: '#322d40',
        fg: '#e6e2f0',
        muted: '#9a92b0',
        accent: '#8b5cf6',
      },
    },
  },
  plugins: [],
};
