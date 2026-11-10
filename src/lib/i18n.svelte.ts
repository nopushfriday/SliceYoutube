import { ko, type Key } from './locales/ko';
import { en } from './locales/en';
import { ja } from './locales/ja';
import { zh } from './locales/zh';
import { zhTW } from './locales/zhTW';
import { es } from './locales/es';
import { fr } from './locales/fr';
import { de } from './locales/de';
import { pt } from './locales/pt';
import { ru } from './locales/ru';
import { it } from './locales/it';
import { id } from './locales/id';
import { vi } from './locales/vi';
import { tr } from './locales/tr';

export const LANGS = [
  { code: 'en', name: 'English' },
  { code: 'ko', name: '한국어' },
  { code: 'ja', name: '日本語' },
  { code: 'zh', name: '简体中文' },
  { code: 'zh-TW', name: '繁體中文' },
  { code: 'es', name: 'Español' },
  { code: 'fr', name: 'Français' },
  { code: 'de', name: 'Deutsch' },
  { code: 'pt', name: 'Português' },
  { code: 'ru', name: 'Русский' },
  { code: 'it', name: 'Italiano' },
  { code: 'id', name: 'Bahasa Indonesia' },
  { code: 'vi', name: 'Tiếng Việt' },
  { code: 'tr', name: 'Türkçe' }
] as const;

export type LangCode = (typeof LANGS)[number]['code'];
export type LangPref = LangCode | 'auto';

const DICTS: Record<LangCode, Record<Key, string>> = { ko, en, ja, zh, 'zh-TW': zhTW, es, fr, de, pt, ru, it, id, vi, tr };
const STORE_KEY = 'sliceyoutube-lang';

/** 시스템 언어(navigator.languages)에서 지원하는 첫 언어를 고른다. 없으면 영어. */
function detect(): LangCode {
  const list = typeof navigator === 'undefined' ? [] : [...(navigator.languages ?? []), navigator.language];
  for (const l of list) {
    const tag = (l ?? '').toLowerCase();
    const base = tag.split('-')[0];
    // 중국어는 번체(대만·홍콩·마카오, Hant)와 간체를 구분한다
    if (base === 'zh') return !tag.includes('hans') && /-(tw|hk|mo|hant)/.test(tag) ? 'zh-TW' : 'zh';
    const hit = LANGS.find((x) => x.code === base);
    if (hit) return hit.code;
  }
  return 'en';
}

function loadPref(): LangPref {
  try {
    const v = localStorage.getItem(STORE_KEY);
    if (v && LANGS.some((x) => x.code === v)) return v as LangCode;
  } catch {
    // 저장소를 못 읽으면 자동 감지로 둔다
  }
  return 'auto';
}

let pref = $state<LangPref>(loadPref());
const code = $derived<LangCode>(pref === 'auto' ? detect() : pref);

export const lang = {
  get pref() {
    return pref;
  },
  get code() {
    return code;
  },
  /** '자동'일 때 실제로 적용되는 언어의 표시 이름 */
  get autoName() {
    return LANGS.find((x) => x.code === detect())!.name;
  },
  set(p: LangPref) {
    pref = p;
    try {
      if (p === 'auto') localStorage.removeItem(STORE_KEY);
      else localStorage.setItem(STORE_KEY, p);
    } catch {
      // 저장하지 못해도 이번 실행에는 적용된다
    }
    document.documentElement.lang = code;
  }
};

/** 번역. {이름} 자리는 params로 채운다. 선택 언어에 키가 없으면 한국어(기준)로 대신한다. */
export function t(key: Key, params?: Record<string, string | number>): string {
  const text = DICTS[code][key] ?? ko[key];
  return params ? text.replace(/\{(\w+)\}/g, (m, k) => (k in params ? String(params[k]) : m)) : text;
}

// 백엔드(Rust)가 한국어로 돌려주는 오류 메시지를 현재 언어로 바꾼다. 모르는 메시지는 그대로 둔다.
const BACKEND: Key[] = [
  'be.cropRange',
  'be.cropSmall',
  'be.exePath',
  'be.ytUpdate',
  'be.streamUrl',
  'be.info',
  'be.download',
  'be.encode',
  'be.endAfterStart',
  'be.fileName',
  'be.convert'
];

export function tb(msg: string | null | undefined): string {
  if (!msg) return '';
  for (const k of BACKEND) if (msg === ko[k]) return t(k);
  if (msg.startsWith(ko['be.ytPrepare'])) return t('be.ytPrepare') + msg.slice(ko['be.ytPrepare'].length);
  return msg;
}
