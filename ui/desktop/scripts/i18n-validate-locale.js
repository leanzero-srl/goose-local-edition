#!/usr/bin/env node
/**
 * Validate that a translated locale catalog mirrors en.json and preserves ICU placeholders.
 */
const fs = require('fs');
const path = require('path');

const locale = process.argv[2] || 'ja';
const projectDir = path.join(__dirname, '..');
const messagesDir = path.join(projectDir, 'src', 'i18n', 'messages');
const enPath = path.join(messagesDir, 'en.json');
const localePath = path.join(messagesDir, locale + '.json');

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function extractPlaceholders(message) {
  const placeholders = new Set();
  const re = /\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?=[},])/g;
  let match;
  while ((match = re.exec(message)) !== null) {
    placeholders.add(match[1]);
  }
  return [...placeholders].sort();
}

const en = readJson(enPath);
const translated = readJson(localePath);
const enKeys = Object.keys(en).sort();
const localeKeys = Object.keys(translated).sort();

const missing = enKeys.filter((key) => !Object.prototype.hasOwnProperty.call(translated, key));
const extra = localeKeys.filter((key) => !Object.prototype.hasOwnProperty.call(en, key));
const placeholderIssues = [];

for (const key of enKeys) {
  if (!translated[key]) continue;
  const source = en[key].defaultMessage || '';
  const target = translated[key].defaultMessage || '';
  const sourcePlaceholders = extractPlaceholders(source);
  const targetPlaceholders = extractPlaceholders(target);
  if (JSON.stringify(sourcePlaceholders) !== JSON.stringify(targetPlaceholders)) {
    placeholderIssues.push({ key, sourcePlaceholders, targetPlaceholders });
  }
}

if (missing.length || extra.length || placeholderIssues.length) {
  if (missing.length) console.error('Missing ' + locale + ' keys:', missing.slice(0, 50));
  if (extra.length) console.error('Extra ' + locale + ' keys:', extra.slice(0, 50));
  if (placeholderIssues.length) console.error('Placeholder issues:', placeholderIssues.slice(0, 20));
  process.exit(1);
}

console.log('i18n locale "' + locale + '" is valid (' + enKeys.length + ' messages).');

