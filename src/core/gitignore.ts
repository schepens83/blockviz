/**
 * A folder opened from disk holds what its `.gitignore` files leave out of
 * the repository: build output, logs, caches, uploaded images. Those are
 * not the project, and in a Rails app they can be thousands of files. This
 * reads the `.gitignore` files of a folder the way git does, so that what
 * blockviz shows of it is what the repository holds.
 */

interface Rule {
  /** The directory whose `.gitignore` it is ('' for the folder itself). */
  base: string;
  negate: boolean;
  /** `logs/`: directories only. */
  dirOnly: boolean;
  re: RegExp;
}

const escape = (c: string) => c.replace(/[.+^${}()|[\]\\]/g, '\\$&');

/** A gitignore glob as a regular expression over a slash-separated path. */
function globSource(glob: string): string {
  let re = '';
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === '*' && glob[i + 1] === '*' && (i === 0 || glob[i - 1] === '/') && (i + 2 === glob.length || glob[i + 2] === '/')) {
      // `**/` is any number of directories; a trailing `/**` everything inside.
      if (i + 2 === glob.length) {
        re += '.*';
        i += 1;
      } else {
        re += '(?:.*/)?';
        i += 2;
      }
    } else if (c === '*') {
      while (glob[i + 1] === '*') i++;
      re += '[^/]*';
    } else if (c === '?') {
      re += '[^/]';
    } else if (c === '[' && glob.indexOf(']', i + 2) > i) {
      const end = glob.indexOf(']', i + 2);
      const set = glob.slice(i + 1, end);
      re += `[${set[0] === '!' ? '^' + set.slice(1) : set}]`;
      i = end;
    } else if (c === '\\' && i + 1 < glob.length) {
      re += escape(glob[++i]);
    } else {
      re += escape(c);
    }
  }
  return re;
}

/** The rules of the `.gitignore` in `base`. */
function parse(text: string, base: string): Rule[] {
  const rules: Rule[] = [];
  for (const raw of text.split(/\r?\n/)) {
    let line = raw.replace(/(?<!\\)\s+$/, '');
    if (!line || line.startsWith('#')) continue;
    const negate = line.startsWith('!');
    if (negate) line = line.slice(1);
    if (line.startsWith('\\')) line = line.slice(1);
    const dirOnly = line.endsWith('/');
    if (dirOnly) line = line.slice(0, -1);
    if (!line) continue;
    // A slash at the start or in the middle ties the pattern to `base`;
    // without one it matches a name at any depth below it.
    const anchored = line.includes('/');
    const glob = line.replace(/^\//, '');
    const re = new RegExp(anchored ? `^${globSource(glob)}$` : `(?:^|/)${globSource(glob)}$`);
    rules.push({ base, negate, dirOnly, re });
  }
  return rules;
}

export interface Ignores {
  /** Whether a file at this path (as the folder lists it) is left out. */
  ignored(path: string): boolean;
  /** How many rules were read. */
  size: number;
}

/**
 * The rules of every `.gitignore` among `files` (as `[path, text]`), for the
 * paths beside and below each. As in git, the last rule that matches wins, a
 * deeper `.gitignore` after a shallower one, and nothing inside an ignored
 * directory comes back.
 */
export function gitignores(files: [string, string][]): Ignores {
  const rules: Rule[] = files
    .filter(([p]) => p === '.gitignore' || p.endsWith('/.gitignore'))
    .sort(([a], [b]) => a.split('/').length - b.split('/').length)
    .flatMap(([p, text]) => parse(text, p.slice(0, Math.max(0, p.length - '.gitignore'.length - 1))));
  const verdict = (path: string, dir: boolean) => {
    let out = false;
    for (const r of rules) {
      if (r.dirOnly && !dir) continue;
      if (r.base && !path.startsWith(r.base + '/')) continue;
      const rel = r.base ? path.slice(r.base.length + 1) : path;
      if (r.re.test(rel)) out = !r.negate;
    }
    return out;
  };
  const dirs = new Map<string, boolean>();
  const dirIgnored = (dir: string): boolean => {
    let v = dirs.get(dir);
    if (v === undefined) {
      const parent = dir.includes('/') ? dir.slice(0, dir.lastIndexOf('/')) : '';
      v = (parent !== '' && dirIgnored(parent)) || verdict(dir, true);
      dirs.set(dir, v);
    }
    return v;
  };
  return {
    size: rules.length,
    ignored(path: string) {
      if (!rules.length) return false;
      const parent = path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';
      return (parent !== '' && dirIgnored(parent)) || verdict(path, false);
    },
  };
}
