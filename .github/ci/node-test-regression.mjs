import fs from 'node:fs';

const [basePath, headPath, baseExitRaw, headExitRaw] = process.argv.slice(2);
if (!basePath || !headPath || baseExitRaw == null || headExitRaw == null) {
  console.error('usage: node-test-regression.mjs <base.log> <head.log> <base-exit> <head-exit>');
  process.exit(64);
}

const baseExit = Number(baseExitRaw);
const headExit = Number(headExitRaw);
if (headExit === 0) {
  console.log('Head test suite is green.');
  process.exit(0);
}
if (baseExit === 0) {
  console.error('Head test suite failed while main is green.');
  process.exit(1);
}

const failures = (path) => {
  const text = fs.readFileSync(path, 'utf8');
  return new Set(
    [...text.matchAll(/^not ok\s+\d+\s+-\s+(.+)$/gm)]
      .map((match) => match[1].trim())
      .filter(Boolean),
  );
};

const baseFailures = failures(basePath);
const headFailures = failures(headPath);
if (headFailures.size === 0) {
  console.error('Head test command failed without TAP failure names; treating this as infrastructure or runner failure.');
  process.exit(1);
}

const regressions = [...headFailures].filter((name) => !baseFailures.has(name));
if (regressions.length) {
  console.error('New failing tests compared with main:');
  for (const name of regressions) console.error('  ' + name);
  process.exit(1);
}

console.log(
  'No new failing TAP tests compared with main. Existing baseline failures remain visible in the uploaded logs.',
);
