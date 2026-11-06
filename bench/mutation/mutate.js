#!/usr/bin/env node
// Mutation generator for Web-Bench reference projects.
//
//   node bench/mutation/mutate.js --src <dir> --out <dir> --project <name> [--max 40] [--seed 1]
//
// Every mutant is a full copy of --src with exactly ONE edit (one changed line,
// one deleted line, or one inserted line for css_hide). Output layout:
//   <out>/<project>/m000/            unmutated control copy
//   <out>/<project>/m001/ ...        mutants
//   <out>/<project>/manifest.json    [{id, operator, file, line, before, after}]
// `file` is relative to --src, `line` is 1-based (line index in the mutant
// where the edit landed), before/after are the changed line, trimmed, <= 200 chars.
//
// No npm dependencies. CommonJS (bench/package.json has no "type" field).
// Written and tested on Node 22.

'use strict';

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { spawnSync } = require('node:child_process');

const OPERATORS = [
  'cond_negate',
  'cmp_flip',
  'num_shift',
  'status_code',
  'string_drop',
  'class_rename',
  'remove_call',
  'css_hide',
  'route_path',
  'await_drop',
  'return_drop',
];

const TEXT_EXT = new Set(['.js', '.pug', '.html', '.css', '.sql']);
const SKIP_DIRS = new Set(['node_modules']);
const SKIP_FILE = (name) => name.endsWith('.sqlite');

// ---------------------------------------------------------------- args

function parseArgs(argv) {
  const opts = { max: 40, seed: 1 };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`missing value for ${a}`);
      return argv[++i];
    };
    switch (a) {
      case '--src': opts.src = next(); break;
      case '--out': opts.out = next(); break;
      case '--project': opts.project = next(); break;
      case '--max': opts.max = Number(next()); break;
      case '--seed': opts.seed = Number(next()); break;
      case '-h':
      case '--help':
        console.log('usage: mutate.js --src <dir> --out <dir> --project <name> [--max 40] [--seed 1]');
        process.exit(0);
        break;
      default:
        throw new Error(`unknown argument: ${a}`);
    }
  }
  for (const k of ['src', 'out', 'project']) {
    if (!opts[k]) throw new Error(`--${k} is required`);
  }
  if (!Number.isInteger(opts.max) || opts.max < 1) throw new Error('--max must be a positive integer');
  if (!Number.isInteger(opts.seed)) throw new Error('--seed must be an integer');
  return opts;
}

// ---------------------------------------------------------------- prng

function mulberry32(seed) {
  let a = seed >>> 0;
  return function next() {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function shuffle(arr, rng) {
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr;
}

// ---------------------------------------------------------------- file walk

function walk(root) {
  const out = [];
  const visit = (dir) => {
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, ent.name);
      if (ent.isDirectory()) {
        if (SKIP_DIRS.has(ent.name)) continue;
        visit(full);
      } else if (ent.isFile()) {
        if (SKIP_FILE(ent.name)) continue;
        out.push(full);
      }
    }
  };
  visit(root);
  return out.sort();
}

function relPosix(root, full) {
  return path.relative(root, full).split(path.sep).join('/');
}

// ---------------------------------------------------------------- JS lexer
//
// Produces a "masked" copy of the source where comments, string contents,
// template text and regex literals are replaced by spaces (newlines kept),
// so structural regexes only ever see code. Also returns string literal
// records (content offsets) so operators can read/edit literal contents.

const REGEX_KEYWORDS = new Set([
  'return', 'typeof', 'case', 'do', 'else', 'in', 'of', 'new', 'delete',
  'void', 'throw', 'instanceof', 'yield', 'await',
]);
const REGEX_PREFIX_PUNCT = new Set(['(', ',', '=', ':', '[', '!', '&', '|', '?', '{', '}', ';', '+', '-', '*', '%', '<', '>', '~', '^']);

function lexJs(src) {
  const n = src.length;
  const m = src.split('');
  const strings = [];
  const stack = []; // {k:'tpl', rec} or {k:'interp', depth}
  const blank = (a, b) => {
    for (let k = a; k < b; k++) if (m[k] !== '\n') m[k] = ' ';
  };
  const isIdent = (c) => /[A-Za-z0-9_$]/.test(c);
  let i = 0;
  let lastTok = '';

  while (i < n) {
    const top = stack.length ? stack[stack.length - 1] : null;

    // template text
    if (top && top.k === 'tpl') {
      const c = src[i];
      if (c === '\\') { blank(i, Math.min(i + 2, n)); i += 2; continue; }
      if (c === '`') {
        stack.pop();
        top.rec.end = i;
        strings.push(top.rec);
        lastTok = 'str';
        i++;
        continue;
      }
      if (c === '$' && src[i + 1] === '{') {
        top.rec.interp = true;
        stack.push({ k: 'interp', depth: 0 });
        lastTok = '{';
        i += 2;
        continue;
      }
      if (c !== '\n') m[i] = ' ';
      i++;
      continue;
    }

    const c = src[i];
    if (/\s/.test(c)) { i++; continue; }

    if (c === '/' && src[i + 1] === '/') {
      const e = src.indexOf('\n', i);
      const end = e < 0 ? n : e;
      blank(i, end);
      i = end;
      continue;
    }
    if (c === '/' && src[i + 1] === '*') {
      const e = src.indexOf('*/', i + 2);
      const end = e < 0 ? n : e + 2;
      blank(i, end);
      i = end;
      continue;
    }
    if (c === "'" || c === '"') {
      let j = i + 1;
      while (j < n && src[j] !== c && src[j] !== '\n') {
        if (src[j] === '\\') j++;
        j++;
      }
      const closed = j < n && src[j] === c;
      strings.push({ start: i + 1, end: j, quote: c, interp: false });
      blank(i + 1, j);
      i = closed ? j + 1 : j;
      lastTok = 'str';
      continue;
    }
    if (c === '`') {
      stack.push({ k: 'tpl', rec: { start: i + 1, end: -1, quote: '`', interp: false } });
      i++;
      continue;
    }
    if (c === '{') {
      if (top && top.k === 'interp') top.depth++;
      lastTok = '{';
      i++;
      continue;
    }
    if (c === '}') {
      if (top && top.k === 'interp') {
        if (top.depth === 0) {
          stack.pop();
          lastTok = '}';
          i++;
          continue;
        }
        top.depth--;
      }
      lastTok = '}';
      i++;
      continue;
    }
    if (c === '/') {
      const regexOk = lastTok === '' || (REGEX_PREFIX_PUNCT.has(lastTok)) || REGEX_KEYWORDS.has(lastTok);
      if (regexOk) {
        let j = i + 1;
        let inCls = false;
        while (j < n && src[j] !== '\n') {
          const d = src[j];
          if (d === '\\') { j += 2; continue; }
          if (inCls) { if (d === ']') inCls = false; }
          else if (d === '[') inCls = true;
          else if (d === '/') break;
          j++;
        }
        if (j < n && src[j] === '/') {
          let e = j + 1;
          while (e < n && /[a-z]/i.test(src[e])) e++;
          blank(i, e);
          i = e;
          lastTok = 'regex';
          continue;
        }
      }
      lastTok = '/';
      i++;
      continue;
    }
    if (isIdent(c)) {
      let j = i;
      while (j < n && isIdent(src[j])) j++;
      lastTok = src.slice(i, j);
      i = j;
      continue;
    }
    lastTok = c;
    i++;
  }
  return { masked: m.join(''), strings };
}

// ---------------------------------------------------------------- helpers

function lineStartsOf(text) {
  const starts = [0];
  for (let k = 0; k < text.length; k++) if (text[k] === '\n') starts.push(k + 1);
  return starts;
}

function matchParen(s, open) {
  let depth = 0;
  for (let k = open; k < s.length; k++) {
    if (s[k] === '(') depth++;
    else if (s[k] === ')') {
      depth--;
      if (depth === 0) return k;
    }
  }
  return -1;
}

function balanced(s) {
  let p = 0, b = 0, c = 0;
  for (const ch of s) {
    if (ch === '(') p++; else if (ch === ')') p--;
    else if (ch === '[') b++; else if (ch === ']') b--;
    else if (ch === '{') c++; else if (ch === '}') c--;
  }
  return p === 0 && b === 0 && c === 0;
}

function clip(s, max = 200) {
  const t = s.trim();
  return t.length <= max ? t : t.slice(0, max - 3) + '...';
}

// A replace-site that rewrites columns [cs, ce) of line li with repl.
function spanSite(op, rel, lines, li, cs, ce, repl) {
  const orig = lines[li];
  const nl = orig.slice(0, cs) + repl + orig.slice(ce);
  return { op, file: rel, line: li, kind: 'replace', newLine: nl, before: clip(orig), after: clip(nl) };
}

function deleteSite(op, rel, lines, li) {
  return { op, file: rel, line: li, kind: 'delete', before: clip(lines[li]), after: '' };
}

function insertSite(op, rel, lines, li, newLine) {
  return { op, file: rel, line: li, kind: 'insert', newLine, before: '', after: clip(newLine) };
}

// Text that looks like user-visible prose: spaces or emoji, letters, and
// no code-ish characters. Used by string_drop for JS, pug and html.
const CODE_CHARS = /[\\{}()=;<>$\/#|]/;
function looksLikeProse(t) {
  if (t.length < 4) return false;
  if (!(/ /.test(t) || /\p{Extended_Pictographic}/u.test(t))) return false;
  if (!/\p{L}{2}/u.test(t)) return false;
  if (CODE_CHARS.test(t)) return false;
  return true;
}

// Returns the text with its last word removed, or null if not applicable.
function dropLastWord(t) {
  const trimmed = t.replace(/\s+$/, '');
  const k = trimmed.lastIndexOf(' ');
  if (k <= 0) return null;
  const head = trimmed.slice(0, k).replace(/\s+$/, '');
  if (!/\p{L}/u.test(head)) return null;
  return head;
}

// Class tokens inside an attribute/string value. `contentStart` is the column of
// the value's first character on the line; yields {start, end, token} columns.
function classTokens(content, contentStart) {
  if (/[$`{}+]/.test(content)) return [];
  const out = [];
  const re = /\S+/g;
  let m;
  while ((m = re.exec(content))) {
    if (!/^[A-Za-z_][\w-]*$/.test(m[0])) continue;
    out.push({ start: contentStart + m.index, end: contentStart + m.index + m[0].length, token: m[0] });
  }
  return out;
}

// ---------------------------------------------------------------- JS operators

function jsSites(rel, text) {
  const { masked, strings } = lexJs(text);
  const lines = text.split('\n');
  const mlines = masked.split('\n');
  const ls = lineStartsOf(text);
  const sites = { cond_negate: [], cmp_flip: [], num_shift: [], status_code: [], string_drop: [], class_rename: [], remove_call: [], route_path: [], await_drop: [], return_drop: [] };

  // cond_negate: if (X) -> if (!(X))
  for (let li = 0; li < mlines.length; li++) {
    const ml = mlines[li];
    const re = /(?<![\w$.])if\s*\(/g;
    let m;
    while ((m = re.exec(ml))) {
      const open = m.index + m[0].length - 1;
      const close = matchParen(ml, open);
      if (close < 0) continue;
      const cond = lines[li].slice(open + 1, close);
      if (!cond.trim()) continue;
      sites.cond_negate.push(spanSite('cond_negate', rel, lines, li, open, close + 1, `(!(${cond}))`));
    }
  }

  // cmp_flip
  const CMP_MAP = { '===': '!==', '!==': '===', '<': '>=', '>': '<=', '>=': '<', '<=': '>' };
  for (let li = 0; li < mlines.length; li++) {
    const ml = mlines[li];
    const re = />>>=|<<=|>>=|!==|===|=>|<<|>>>|>>|<=|>=|<|>/g;
    let m;
    while ((m = re.exec(ml))) {
      const tok = m[0];
      if (!(tok in CMP_MAP)) continue;
      sites.cmp_flip.push(spanSite('cmp_flip', rel, lines, li, m.index, m.index + tok.length, CMP_MAP[tok]));
    }
  }

  // num_shift: integer literal n (not 0) -> n+1
  for (let li = 0; li < mlines.length; li++) {
    const ml = mlines[li];
    const re = /(?<![\w$.])(\d+)(?![\w.])/g;
    let m;
    while ((m = re.exec(ml))) {
      if (m[1] === '0') continue;
      if (/\.status\(\s*$/.test(ml.slice(0, m.index))) continue; // status_code owns these
      sites.num_shift.push(spanSite('num_shift', rel, lines, li, m.index, m.index + m[1].length, String(Number(m[1]) + 1)));
    }
  }

  // status_code: .status(2xx) -> 500, .status(4xx) -> 200
  for (let li = 0; li < mlines.length; li++) {
    const ml = mlines[li];
    const re = /\.status\(\s*(\d{3})\s*\)/g;
    let m;
    while ((m = re.exec(ml))) {
      const code = m[1];
      const digitsAt = m.index + m[0].indexOf(code);
      if (code[0] === '2') sites.status_code.push(spanSite('status_code', rel, lines, li, digitsAt, digitsAt + 3, '500'));
      else if (code[0] === '4') sites.status_code.push(spanSite('status_code', rel, lines, li, digitsAt, digitsAt + 3, '200'));
    }
  }

  // string_drop: delete last word of a user-visible literal
  const SKIP_LINE = /console\.|require\(|\bimport\b|\.setItem\(|\.getItem\(|querySelector|\bfetch\(/;
  for (const rec of strings) {
    if (rec.interp || rec.end < 0) continue;
    const content = text.slice(rec.start, rec.end);
    if (content.includes('\n')) continue;
    if (content.trim() === 'use strict') continue;
    const li = lineIndexOf(ls, rec.start);
    const cs = rec.start - ls[li];
    if (SKIP_LINE.test(mlines[li])) continue;
    const t = content.trim();
    if (!looksLikeProse(t)) continue;
    const head = dropLastWord(t);
    if (head === null) continue;
    const lead = content.length - content.trimStart().length;
    sites.string_drop.push(spanSite('string_drop', rel, lines, li, cs + lead, cs + lead + t.length, head));
  }

  // class_rename: class="..." attributes (markup or JS string) and classList/className in JS
  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    const re = /\bclass\s*=\s*(["'])([^"'\n]*)\1/g;
    let m;
    while ((m = re.exec(line))) {
      const valStart = m.index + m[0].length - m[2].length - 1;
      for (const t of classTokens(m[2], valStart)) {
        sites.class_rename.push(spanSite('class_rename', rel, lines, li, t.start, t.end, t.token + '-x'));
      }
    }
    const re2 = /\.className\s*=\s*(['"])([^'"\n]*)\1/g;
    while ((m = re2.exec(line))) {
      if (!/^\./.test(masked.slice(ls[li] + m.index, ls[li] + m.index + 1))) continue;
      const valStart = m.index + m[0].length - m[2].length - 1;
      for (const t of classTokens(m[2], valStart)) {
        sites.class_rename.push(spanSite('class_rename', rel, lines, li, t.start, t.end, t.token + '-x'));
      }
    }
    const re3 = /\bclassList\.(add|remove|toggle)\(([^)\n]*)\)/g;
    while ((m = re3.exec(line))) {
      if (masked.slice(ls[li] + m.index, ls[li] + m.index + 9) !== 'classList') continue;
      const argsStart = m.index + m[0].indexOf('(') + 1;
      const argRe = /(['"])([^'"\n]*)\1/g;
      let a;
      while ((a = argRe.exec(m[2]))) {
        const valStart = argsStart + a.index + 1;
        for (const t of classTokens(a[2], valStart)) {
          sites.class_rename.push(spanSite('class_rename', rel, lines, li, t.start, t.end, t.token + '-x'));
        }
      }
    }
  }

  // remove_call: delete a whole statement line that is a DOM/response/storage call
  const CALL_RE = /^\s*(?:[\w$]+\.)+(?:appendChild|prepend|cookie|setItem|addEventListener)\s*\(/;
  for (let li = 0; li < lines.length; li++) {
    const ml = mlines[li];
    if (!CALL_RE.test(ml)) continue;
    if (!balanced(ml)) continue;
    const orig = lines[li].trim();
    if (!orig) continue;
    let prevIdx = li - 1;
    while (prevIdx >= 0 && !mlines[prevIdx].trim()) prevIdx--;
    if (prevIdx >= 0 && /[=(,&|+?:]$/.test(mlines[prevIdx].trim())) continue;
    let nextIdx = li + 1;
    while (nextIdx < mlines.length && !mlines[nextIdx].trim()) nextIdx++;
    const endsStmt = orig.endsWith(';');
    const nextStarts = nextIdx < mlines.length ? mlines[nextIdx].trim()[0] : '';
    if (!endsStmt && (!orig.endsWith(')') || /[.(\[`+\-\/*]/.test(nextStarts))) continue;
    sites.remove_call.push(deleteSite('remove_call', rel, lines, li));
  }

  // route_path: router.get|post('/path') -> '/paths'
  const recByStart = new Map(strings.map((r) => [r.start, r]));
  for (let li = 0; li < mlines.length; li++) {
    const ml = mlines[li];
    const re = /\brouter\.(?:get|post|put|patch|delete)\s*\(\s*(?=['"])/g;
    let m;
    while ((m = re.exec(ml))) {
      const qpos = m.index + m[0].length;
      const rec = recByStart.get(ls[li] + qpos + 1);
      if (!rec || rec.interp || rec.end < 0) continue;
      const p = text.slice(rec.start, rec.end);
      if (!/^\/[\w\/:.\-]*$/.test(p)) continue;
      const last = p.split('/').pop();
      if (!last) continue;
      const cs = rec.start - ls[li];
      sites.route_path.push(spanSite('route_path', rel, lines, li, cs, cs + p.length, p + 's'));
    }
  }

  // await_drop: remove `await ` (not `for await`)
  for (let li = 0; li < mlines.length; li++) {
    const ml = mlines[li];
    const re = /(?<![\w$.])await\s+(?=[\w$(])/g;
    let m;
    while ((m = re.exec(ml))) {
      if (/\bfor\s*$/.test(ml.slice(0, m.index))) continue;
      sites.await_drop.push(spanSite('await_drop', rel, lines, li, m.index, m.index + m[0].length, ''));
    }
  }

  // return_drop: `return expr;` at statement start -> `expr;`
  for (let li = 0; li < mlines.length; li++) {
    const ml = mlines[li];
    const m = /^(\s*)return(\s+)(?=\S)/.exec(ml);
    if (!m) continue;
    // Statement must end cleanly: `;`, or an expression tail (the code base mostly omits semicolons).
    const tail = lines[li].trim();
    if (!/[;\w)\]'"`}]$/.test(tail)) continue;
    if (!balanced(ml)) continue;
    let nextIdx = li + 1;
    while (nextIdx < mlines.length && !mlines[nextIdx].trim()) nextIdx++;
    if (nextIdx < mlines.length && /^[.(\[`+\-\/*?:]/.test(mlines[nextIdx].trim())) continue;
    const cs = m[1].length;
    sites.return_drop.push(spanSite('return_drop', rel, lines, li, cs, cs + 'return'.length + m[2].length, ''));
  }

  return sites;
}

function lineIndexOf(starts, off) {
  let lo = 0, hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= off) lo = mid; else hi = mid - 1;
  }
  return lo;
}

// ---------------------------------------------------------------- markup operators (pug / html)

const PUG_KEYWORDS = new Set(['doctype', 'extends', 'block', 'include', 'if', 'else', 'unless', 'each', 'for', 'while', 'case', 'when', 'default', 'mixin', 'append', 'prepend', 'script', 'style', 'var', 'let', 'const', 'return', 'yield']);

function htmlSites(rel, text, kind) {
  const lines = text.split('\n');
  const sites = { string_drop: [], class_rename: [] };

  // class_rename on class="..." attributes
  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    const re = /\bclass\s*=\s*(["'])([^"'\n]*)\1/g;
    let m;
    while ((m = re.exec(line))) {
      const valStart = m.index + m[0].length - m[2].length - 1;
      for (const t of classTokens(m[2], valStart)) {
        sites.class_rename.push(spanSite('class_rename', rel, lines, li, t.start, t.end, t.token + '-x'));
      }
    }
  }

  if (kind === 'html') {
    // text nodes between > and <, skipping <script> and <style> bodies
    let inRaw = false;
    for (let li = 0; li < lines.length; li++) {
      const line = lines[li];
      if (/<(script|style)\b/i.test(line) && !/<\/(script|style)>/i.test(line)) inRaw = true;
      const wasRaw = inRaw;
      if (/<\/(script|style)>/i.test(line)) inRaw = false;
      if (wasRaw) continue;
      const re = />([^<>{}\n]+)(?=<)/g;
      let m;
      while ((m = re.exec(line))) {
        const content = m[1];
        const t = content.trim();
        if (!looksLikeProse(t)) continue;
        const head = dropLastWord(t);
        if (head === null) continue;
        const cs = m.index + 1 + (content.length - content.trimStart().length);
        sites.string_drop.push(spanSite('string_drop', rel, lines, li, cs, cs + t.length, head));
      }
    }
    return sites;
  }

  // pug
  let scriptIndent = -1;
  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    if (!line.trim()) continue;
    const indent = line.match(/^\s*/)[0].length;
    if (scriptIndent >= 0) {
      if (indent > scriptIndent) continue;
      scriptIndent = -1;
    }
    if (/^\s*(script|style)(\.|\s*$)/.test(line)) { scriptIndent = indent; continue; }
    const trimmed = line.trim();
    if (trimmed.startsWith('//') || trimmed.startsWith('-') || trimmed.startsWith('+') || trimmed.startsWith('include') || trimmed.startsWith('extends')) continue;

    let m = /^(\s*\|\s?)(.+)$/.exec(line);
    if (m) {
      const t = m[2].trim();
      if (!/#\{|!\{/.test(t) && looksLikeProse(t)) {
        const head = dropLastWord(t);
        if (head !== null) {
          const cs = m[1].length + (m[2].length - m[2].trimStart().length);
          sites.string_drop.push(spanSite('string_drop', rel, lines, li, cs, cs + t.length, head));
        }
      }
      continue;
    }

    m = /^(\s*)([a-zA-Z][\w-]*(?:[.#][\w-]+)*(?:\([^)]*\))?)(\s+)(\S.*)$/.exec(line);
    if (m) {
      const tag = m[2].match(/^[a-zA-Z][\w-]*/)[0];
      if (PUG_KEYWORDS.has(tag)) continue;
      const t = m[4].trim();
      if (/#\{|!\{|^=|^-/.test(t)) continue;
      if (!looksLikeProse(t)) continue;
      const head = dropLastWord(t);
      if (head === null) continue;
      const cs = m[1].length + m[2].length + m[3].length;
      sites.string_drop.push(spanSite('string_drop', rel, lines, li, cs, cs + t.length, head));
    }
  }
  return sites;
}

// ---------------------------------------------------------------- CSS operator

function collectMarkupClasses(files, root) {
  const classes = new Set();
  const addTokens = (s) => {
    for (const t of s.split(/\s+/)) if (/^[A-Za-z_][\w-]*$/.test(t)) classes.add(t);
  };
  for (const f of files) {
    const ext = path.extname(f);
    if (!['.js', '.pug', '.html'].includes(ext)) continue;
    const text = fs.readFileSync(f, 'utf8');
    for (const m of text.matchAll(/\bclass\s*=\s*(["'])([^"'\n]*)\1/g)) addTokens(m[2]);
    for (const m of text.matchAll(/classList\.(?:add|remove|toggle|contains)\(([^)\n]*)\)/g)) {
      for (const a of m[1].matchAll(/(['"])([^'"\n]*)\1/g)) addTokens(a[2]);
    }
    for (const m of text.matchAll(/\.className\s*=\s*(['"])([^'"\n]*)\1/g)) addTokens(m[2]);
    for (const m of text.matchAll(/querySelector(?:All)?\(\s*(['"])([^'"\n]*)\1/g)) {
      for (const c of m[2].matchAll(/\.([A-Za-z_][\w-]*)/g)) classes.add(c[1]);
    }
    if (ext === '.pug') {
      for (const m of text.matchAll(/(?:^|[\s(])[a-zA-Z]*((?:\.[A-Za-z_][\w-]*)+)/gm)) {
        for (const c of m[1].matchAll(/\.([A-Za-z_][\w-]*)/g)) classes.add(c[1]);
      }
    }
  }
  return classes;
}

function cssSites(rel, text, classes) {
  const lines = text.split('\n');
  const sites = [];
  for (let li = 0; li < lines.length; li++) {
    const trimmed = lines[li].trim();
    const m = /^([^{}@\/*]+?)\s*\{$/.exec(trimmed);
    if (!m) continue;
    const sel = m[1].trim();
    const cm = /^\.([A-Za-z_][\w-]*)$/.exec(sel);
    if (!cm || !classes.has(cm[1])) continue;
    let close = -1;
    let bad = false;
    for (let j = li + 1; j < lines.length; j++) {
      const t = lines[j].trim();
      if (t === '}') { close = j; break; }
      if (t.includes('{') || t.includes('}') || /display\s*:/.test(t)) { bad = true; break; }
    }
    if (close < 0 || bad) continue;
    const indentSrc = close > li + 1 ? lines[li + 1] : lines[li];
    const indent = indentSrc.match(/^\s*/)[0] || '  ';
    sites.push(insertSite('css_hide', rel, lines, close, `${indent}display: none;`));
  }
  return sites;
}

// ---------------------------------------------------------------- syntax guard

function jsParses(text, isModule) {
  if (!isModule) {
    try {
      new vm.Script(text);
      return true;
    } catch (e) {
      return false;
    }
  }
  const probe = `new (require('vm').SourceTextModule)(require('fs').readFileSync(0, 'utf8'));`;
  const r = spawnSync(process.execPath, ['--no-warnings', '--experimental-vm-modules', '-e', probe], {
    input: text,
    encoding: 'utf8',
  });
  return r.status === 0;
}

function looksEsm(text) {
  return /^\s*(import\s|export\s)/m.test(text);
}

// ---------------------------------------------------------------- apply / verify

function applyEdit(lines, site) {
  const out = lines.slice();
  if (site.kind === 'replace') out[site.line] = site.newLine;
  else if (site.kind === 'delete') out.splice(site.line, 1);
  else if (site.kind === 'insert') out.splice(site.line, 0, site.newLine);
  else throw new Error(`unknown edit kind ${site.kind}`);
  return out;
}

function verifyEdit(orig, mut, site) {
  if (site.kind === 'replace') {
    if (mut.length !== orig.length) throw new Error('replace changed line count');
    const diffs = [];
    for (let i = 0; i < orig.length; i++) if (orig[i] !== mut[i]) diffs.push(i);
    if (diffs.length !== 1 || diffs[0] !== site.line) throw new Error(`replace touched ${diffs.length} lines`);
  } else if (site.kind === 'delete') {
    if (mut.length !== orig.length - 1) throw new Error('delete line count mismatch');
    const rebuilt = orig.slice(0, site.line).concat(orig.slice(site.line + 1));
    if (rebuilt.join('\n') !== mut.join('\n')) throw new Error('delete did not remove exactly one line');
  } else if (site.kind === 'insert') {
    if (mut.length !== orig.length + 1) throw new Error('insert line count mismatch');
    const rebuilt = mut.slice(0, site.line).concat(mut.slice(site.line + 1));
    if (rebuilt.join('\n') !== orig.join('\n')) throw new Error('insert did not add exactly one line');
  }
  if (orig.join('\n') === mut.join('\n')) throw new Error('mutant is identical to original');
}

// ---------------------------------------------------------------- main

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const src = path.resolve(opts.src);
  const outProject = path.join(path.resolve(opts.out), opts.project);
  if (!fs.statSync(src).isDirectory()) throw new Error(`--src is not a directory: ${src}`);

  const allFiles = walk(src);
  const textFiles = allFiles.filter((f) => TEXT_EXT.has(path.extname(f)));
  const classes = collectMarkupClasses(textFiles, src);

  // Candidate sites, grouped by operator then file.
  const byOp = Object.fromEntries(OPERATORS.map((op) => [op, new Map()]));
  const addSite = (site) => {
    if (!byOp[site.op].has(site.file)) byOp[site.op].set(site.file, []);
    byOp[site.op].get(site.file).push(site);
  };
  const fileText = new Map();
  for (const f of textFiles) {
    const rel = relPosix(src, f);
    const text = fs.readFileSync(f, 'utf8');
    fileText.set(rel, text);
    const ext = path.extname(f);
    if (ext === '.js') {
      const s = jsSites(rel, text);
      for (const op of Object.keys(s)) s[op].forEach(addSite);
    } else if (ext === '.pug' || ext === '.html') {
      const s = htmlSites(rel, text, ext === '.pug' ? 'pug' : 'html');
      s.string_drop.forEach(addSite);
      s.class_rename.forEach(addSite);
    } else if (ext === '.css') {
      cssSites(rel, text, classes).forEach(addSite);
    }
  }

  const candidateCounts = Object.fromEntries(OPERATORS.map((op) => [op, [...byOp[op].values()].reduce((a, b) => a + b.length, 0)]));

  // Deterministic queue per operator: shuffle files and sites with the seed, then interleave files.
  const rng = mulberry32(opts.seed);
  const queues = {};
  for (const op of OPERATORS) {
    const files = [...byOp[op].keys()].sort();
    shuffle(files, rng);
    const perFile = files.map((f) => shuffle(byOp[op].get(f).slice().sort((a, b) => a.line - b.line), rng));
    const q = [];
    for (let round = 0; ; round++) {
      let any = false;
      for (const list of perFile) {
        if (round < list.length) { q.push(list[round]); any = true; }
      }
      if (!any) break;
    }
    queues[op] = q;
  }

  // Syntax guard cache: original parses? (if the source itself does not parse, skip the guard for that file)
  const origParse = new Map();
  const parseOkFor = (rel, text) => {
    if (!rel.endsWith('.js')) return true;
    const esm = looksEsm(text);
    if (!origParse.has(rel)) origParse.set(rel, jsParses(fileText.get(rel), esm));
    if (!origParse.get(rel)) return true;
    return jsParses(text, esm);
  };

  // Round-robin over operators until --max mutants or queues run dry.
  const picked = [];
  const cursor = Object.fromEntries(OPERATORS.map((op) => [op, 0]));
  let progress = true;
  while (picked.length < opts.max && progress) {
    progress = false;
    for (const op of OPERATORS) {
      if (picked.length >= opts.max) break;
      while (cursor[op] < queues[op].length) {
        const site = queues[op][cursor[op]++];
        const orig = fileText.get(site.file).split('\n');
        const mutLines = applyEdit(orig, site);
        const mutText = mutLines.join('\n');
        if (!parseOkFor(site.file, mutText)) continue; // syntax guard: drop breaking mutants
        picked.push(site);
        progress = true;
        break;
      }
    }
  }

  // Write output.
  fs.rmSync(outProject, { recursive: true, force: true });
  fs.mkdirSync(outProject, { recursive: true });
  const copyTree = (dest) => fs.cpSync(src, dest, {
    recursive: true,
    filter: (p) => {
      const base = path.basename(p);
      if (SKIP_DIRS.has(base) && fs.statSync(p).isDirectory()) return false;
      if (SKIP_FILE(base)) return false;
      return true;
    },
  });

  // Control.
  copyTree(path.join(outProject, 'm000'));

  const manifest = [];
  picked.forEach((site, idx) => {
    const id = `m${String(idx + 1).padStart(3, '0')}`;
    const dir = path.join(outProject, id);
    copyTree(dir);
    const orig = fileText.get(site.file).split('\n');
    const mutLines = applyEdit(orig, site);
    verifyEdit(orig, mutLines, site);
    const target = path.join(dir, ...site.file.split('/'));
    fs.writeFileSync(target, mutLines.join('\n'));
    // Re-read from disk and confirm exactly the intended edit landed.
    const onDisk = fs.readFileSync(target, 'utf8').split('\n');
    verifyEdit(orig, onDisk, site);
    manifest.push({
      id,
      operator: site.op,
      file: site.file,
      line: site.line + 1,
      before: site.before,
      after: site.after,
    });
  });
  fs.writeFileSync(path.join(outProject, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

  // Summary.
  const opCounts = Object.fromEntries(OPERATORS.map((op) => [op, 0]));
  for (const s of picked) opCounts[s.op]++;
  console.log(`project=${opts.project} seed=${opts.seed} src=${src}`);
  console.log(`files scanned: ${textFiles.length} text, ${allFiles.length} total`);
  console.log('');
  console.log('operator        candidates  mutants');
  console.log('--------------  ----------  -------');
  for (const op of OPERATORS) {
    console.log(`${op.padEnd(14)}  ${String(candidateCounts[op]).padStart(10)}  ${String(opCounts[op]).padStart(7)}`);
  }
  console.log('--------------  ----------  -------');
  console.log(`${'total'.padEnd(14)}  ${String(Object.values(candidateCounts).reduce((a, b) => a + b, 0)).padStart(10)}  ${String(picked.length).padStart(7)}`);
  console.log(`control: ${path.join(outProject, 'm000')}`);
  console.log(`manifest: ${path.join(outProject, 'manifest.json')}`);
}

try {
  main();
} catch (err) {
  console.error(`mutate.js: ${err.message}`);
  process.exit(1);
}
