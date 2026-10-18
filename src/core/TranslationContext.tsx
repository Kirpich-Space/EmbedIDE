import { createContext, useContext, useMemo, type ReactNode } from 'react'
import { getFlatTranslations, DEFAULT_LANG, type LangCode } from './translations'

export type TFunction = (key: string, params?: Record<string, string | number>) => string

interface TranslationCtx {
  lang: LangCode
  t: TFunction
}

const Ctx = createContext<TranslationCtx>({ lang: DEFAULT_LANG, t: (k: string) => k })

export function useTranslation() {
  return useContext(Ctx)
}

interface TranslationProviderProps {
  lang: LangCode
  children: ReactNode
}

export function TranslationProvider({ lang, children }: TranslationProviderProps) {
  const ctx = useMemo<TranslationCtx>(() => {
    const dict = getFlatTranslations(lang)
    const t: TFunction = (key, params) => {
      let val = dict[key] ?? key
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          val = val.replace(`{${k}}`, String(v))
        }
      }
      return val
    }
    return { t, lang }
  }, [lang])

  return <Ctx.Provider value={ctx}>{children}</Ctx.Provider>
}
