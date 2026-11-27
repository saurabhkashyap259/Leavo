import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from '../../../locales/en.json'

// All interface text comes from locales/*.json (FR-054). English is the only v1 language.
void i18n.use(initReactI18next).init({
  resources: { en: { translation: en } },
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
})

export default i18n
