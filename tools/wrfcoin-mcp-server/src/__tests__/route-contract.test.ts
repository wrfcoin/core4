import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import ts from 'typescript';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const manifest = JSON.parse(readFileSync(resolve(root, 'src/routes.allowlist.json'), 'utf8')) as {
  node: { method: string; path: string }[];
  backend: { service: string; method: string; path: string; mount: string | null; file: string; needle: string }[];
};
const normalize = (path: string) => path.replace(/\{[^}]*\}/g, '{}');

function check(source: string) {
  let count = 0;
  const parsed = ts.createSourceFile('tool.ts', source, ts.ScriptTarget.Latest, true);
  function walk(node: ts.Node) {
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)) {
      const method = node.expression.name.text.toUpperCase();
      const receiver = node.expression.expression;
      const clientReceiver = ts.isPropertyAccessExpression(receiver) && receiver.expression.getText(parsed) === 'client';
      const requestMethod = ['GET', 'POST', 'PUT', 'DELETE', 'PATCH'].includes(method);
      if (requestMethod || clientReceiver) {
        assert.ok(clientReceiver && requestMethod, `uncheckable request receiver/method: ${node.expression.getText(parsed)}`);
        count++;
        const service = (receiver as ts.PropertyAccessExpression).name.text;
        const arg = node.arguments[0];
        let path: string;
        if (arg && (ts.isStringLiteral(arg) || ts.isNoSubstitutionTemplateLiteral(arg))) path = arg.text;
        else if (arg && ts.isTemplateExpression(arg)) path = arg.head.text + arg.templateSpans.map(span => '{}' + span.literal.text).join('');
        else throw new Error(`uncheckable ${service} ${method} path`);
        const routes = service === 'chain' ? manifest.node : manifest.backend.filter(route => route.service === service);
        assert.ok(routes.some(route => route.method === method && normalize(route.path) === normalize(path)), `unlisted ${service} ${method} ${path}`);
      }
    }
    ts.forEachChild(node, walk);
  }
  walk(parsed);
  return count;
}

test('every tool/resource request is in the target and method allowlist', () => {
  const sourceRoot = process.env.MCP_ROUTE_SOURCE ?? resolve(root, 'src');
  let requests = 0;
  for (const dir of ['tools', 'resources']) for (const file of readdirSync(resolve(sourceRoot, dir))) {
    if (file.endsWith('.ts')) requests += check(readFileSync(resolve(sourceRoot, dir, file), 'utf8'));
  }
  assert.ok(requests >= 12, `vacuous inventory: ${requests}`);
});

test('every allowed node method/path is registered by launch_api', () => {
  const router = readFileSync(resolve(root, '../../native-chain/src/bin/launch_api/routes/mod.rs'), 'utf8');
  const routes = [...router.matchAll(/\.route\(\s*"([^"]+)"\s*,\s*web::(get|post|put|delete)\(\)/g)].map(match => ({ path: normalize(match[1]), method: match[2].toUpperCase() }));
  for (const route of manifest.node) assert.ok(routes.some(actual => actual.method === route.method && actual.path === normalize(route.path)), `node route drift: ${route.method} ${route.path}`);
});

test('an unserved path or wrong HTTP method trips the gate', () => {
  assert.throws(() => check("client.chain.get('/synthetic/unserved')"), /unlisted/);
  assert.throws(() => check("client.chain.post('/readyz')"), /unlisted/);
  assert.throws(() => check('client.chain.get(path)'), /uncheckable/);
  assert.throws(() => check("alias.get('/synthetic/unserved')"), /uncheckable/);
  assert.throws(() => check("client.chain.request({url: '/synthetic/unserved'})"), /uncheckable/);
});

test('backend allowlist still has mounts/handlers on refreshed main', { skip: !process.env.BACKEND_REPO }, () => {
  const git = (file: string) => execFileSync('git', ['-C', process.env.BACKEND_REPO!, 'show', `origin/main:${file}`], { encoding: 'utf8' });
  const entrypoint = git('server/enhanced_unified_api_optimized.ts');
  for (const route of manifest.backend) {
    if (route.mount) assert.ok(entrypoint.includes(`safeMount('${route.mount}'`), `backend mount drift: ${route.mount}`);
    assert.ok(git(route.file).includes(route.needle), `backend handler drift: ${route.path}`);
  }
});
