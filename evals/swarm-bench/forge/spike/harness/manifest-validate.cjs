// @forge/manifest programmatic validation (the schema ships in the package: out/schema/manifest-schema.json).
// Usage: node manifest-validate.cjs <appDir>  -- cwd must be the app (validators resolve handlers/resources from cwd).
const path = require('path');
const { validate } = require('@forge/manifest');
(async () => {
  process.chdir(path.resolve(process.argv[2]));
  const r = await validate(false); // process(undefined) reads ./manifest.yml
  console.log(JSON.stringify({ success: r.success, errors: (r.errors ?? []).map((e) => `${e.level}: ${e.message} @${e.reference ?? ''}:${e.line ?? ''}`) }, null, 1));
})().catch((e) => { console.error('CRASH', e); process.exit(2); });
