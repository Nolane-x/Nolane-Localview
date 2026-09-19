import fs from 'node:fs/promises';
import { createRequire } from 'node:module';
import process from 'node:process';

const appRequire = createRequire(new URL('../../apps/desktop/package.json', import.meta.url));
const ts = appRequire('typescript');

const sourcePath = new URL('../../apps/desktop/src/i18n.ts', import.meta.url);
const source = await fs.readFile(sourcePath, 'utf8');
const transpiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
  fileName: 'i18n.ts',
}).outputText;

const moduleUrl = `data:text/javascript;base64,${Buffer.from(transpiled, 'utf8').toString('base64')}`;
const i18n = await import(moduleUrl);

const {
  DEFAULT_LOCALE,
  PRIMARY_FLOW_MESSAGE_KEYS,
  SUPPORTED_LOCALES,
  localeIntegrityReport,
  messages,
} = i18n;

function invariant(condition, message, details = {}) {
  if (!condition) {
    process.stderr.write(`locale integrity failed: ${message} :: ${JSON.stringify(details)}\n`);
    process.exit(1);
  }
}

invariant(DEFAULT_LOCALE === 'en', 'English must remain the canonical fallback', { DEFAULT_LOCALE });
invariant(Array.isArray(SUPPORTED_LOCALES), 'SUPPORTED_LOCALES must be an array');
invariant(SUPPORTED_LOCALES.length === 12, 'all 12 supported locales must remain registered', {
  supported: SUPPORTED_LOCALES,
});

const registeredDictionaries = Object.keys(messages).sort();
const registeredLocales = [...SUPPORTED_LOCALES].sort();
invariant(
  JSON.stringify(registeredDictionaries) === JSON.stringify(registeredLocales),
  'dictionary registry must exactly match SUPPORTED_LOCALES',
  { registeredDictionaries, registeredLocales },
);

invariant(
  Array.isArray(PRIMARY_FLOW_MESSAGE_KEYS) && PRIMARY_FLOW_MESSAGE_KEYS.length > 0,
  'PRIMARY_FLOW_MESSAGE_KEYS must contain canonical keys',
);
invariant(
  PRIMARY_FLOW_MESSAGE_KEYS.every((key) => typeof messages.en[key] === 'string' && messages.en[key].trim()),
  'every canonical primary-flow key must have a non-empty English fallback',
);

const report = localeIntegrityReport();
invariant(
  Array.isArray(report.missingEnglishFallbackKeys) && report.missingEnglishFallbackKeys.length === 0,
  'English primary-flow fallback must be complete',
  { missingEnglishFallbackKeys: report.missingEnglishFallbackKeys },
);

for (const locale of SUPPORTED_LOCALES) {
  const entry = report.locales.find((candidate) => candidate.locale === locale);
  invariant(Boolean(entry), 'localeIntegrityReport must include every supported locale', { locale });
  invariant(
    Array.isArray(entry.missingPrimaryKeys) && entry.missingPrimaryKeys.length === 0,
    'locale must contain every primary-flow key',
    { locale, missingPrimaryKeys: entry.missingPrimaryKeys },
  );
  invariant(
    Array.isArray(entry.emptyPrimaryKeys) && entry.emptyPrimaryKeys.length === 0,
    'locale primary-flow strings must not be empty',
    { locale, emptyPrimaryKeys: entry.emptyPrimaryKeys },
  );
}

process.stdout.write(
  `localization integrity OK: ${SUPPORTED_LOCALES.length} locales × ${PRIMARY_FLOW_MESSAGE_KEYS.length} primary keys\n`,
);
