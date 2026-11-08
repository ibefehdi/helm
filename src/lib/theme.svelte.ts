export type ThemeName = 'light' | 'dark';

const KEY = 'helm.theme';

export function preferredTheme(): ThemeName {
  if (typeof localStorage !== 'undefined') {
    const saved = localStorage.getItem(KEY);
    if (saved === 'light' || saved === 'dark') return saved;
  }
  if (typeof matchMedia !== 'undefined' && matchMedia('(prefers-color-scheme: dark)').matches) return 'dark';
  return 'light';
}

export function applyTheme(theme: ThemeName) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.classList.toggle('dark', theme === 'dark');
  root.style.colorScheme = theme;
  document.querySelector('meta[name="color-scheme"]')?.setAttribute('content', theme);
}

class ThemeStore {
  current = $state<ThemeName>('light');

  sync() {
    const next = preferredTheme();
    this.current = next;
    applyTheme(next);
  }

  set(next: ThemeName) {
    this.current = next;
    try {
      localStorage.setItem(KEY, next);
    } catch {
      /* Storage can be blocked; the theme still applies for this visit. */
    }
    applyTheme(next);
  }
}

export const theme = new ThemeStore();
