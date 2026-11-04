/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** `1` = Klickdummy einbetten (Mock statt Server). */
  readonly VITE_MOCK?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
