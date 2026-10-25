import { describe, expect, it } from 'vitest';
import { gitignores } from './gitignore';

describe('gitignore', () => {
  it('leaves out what a Rails app ignores, and keeps what it brings back', () => {
    const ig = gitignores([['app/.gitignore', [
      '# comments and blank lines are not rules',
      '',
      '/log/*',
      '!/log/.keep',
      '/tmp',
      '/public/**/**',
      '/app/assets/images/**/**',
      '!/app/assets/images/*flag*',
      '!/app/assets/images/decoration/',
      '!/app/assets/images/decoration/icon.png',
      '.byebug_history',
      'yarn-debug.log*',
      'coverage/',
    ].join('\n')]]);
    const out = (p: string) => ig.ignored(`app/${p}`);
    expect(out('log/development.log')).toBe(true);
    expect(out('log/.keep')).toBe(false);
    expect(out('tmp/cache/bootsnap/compile-cache/00/1a')).toBe(true);
    expect(out('public/assets/app-1.css')).toBe(true);
    expect(out('public/robots.txt')).toBe(true);
    expect(out('app/assets/images/scraps/1.jpg')).toBe(true);
    expect(out('app/assets/images/nl_flag.png')).toBe(false);
    expect(out('app/assets/images/decoration/icon.png')).toBe(false);
    expect(out('app/assets/images/decoration/other.png')).toBe(true);
    // A name without a slash matches at any depth; `coverage/` only a directory.
    expect(out('.byebug_history')).toBe(true);
    expect(out('lib/.byebug_history')).toBe(true);
    expect(out('yarn-debug.log.1')).toBe(true);
    expect(out('coverage/index.html')).toBe(true);
    expect(out('spec/coverage')).toBe(false);
    // The code stays.
    for (const p of ['app/models/user.rb', 'config/routes.rb', 'Gemfile', 'lib/tasks/import.rake', 'app/views/stories/show.html.erb']) {
      expect(out(p)).toBe(false);
    }
  });

  it('applies a nested .gitignore below its own directory, after the outer one', () => {
    const ig = gitignores([
      ['.gitignore', '*.log\nbuild/\n'],
      ['web/.gitignore', '!keep.log\ndist\n'],
    ]);
    expect(ig.ignored('server.log')).toBe(true);
    expect(ig.ignored('web/keep.log')).toBe(false);
    expect(ig.ignored('web/other.log')).toBe(true);
    expect(ig.ignored('web/dist/index.js')).toBe(true);
    expect(ig.ignored('dist/index.js')).toBe(false);
    expect(ig.ignored('src/build/out.js')).toBe(true);
  });

  it('cannot bring back a file inside an ignored directory, as git cannot', () => {
    const ig = gitignores([['.gitignore', 'vendor/\n!vendor/keep.rb\n']]);
    expect(ig.ignored('vendor/keep.rb')).toBe(true);
  });

  it('reads globs: ?, classes, ** in the middle, escapes', () => {
    const ig = gitignores([['.gitignore', 'file?.txt\n*.[oa]\ndocs/**/draft.md\n\\#notes\n']]);
    expect(ig.ignored('file1.txt')).toBe(true);
    expect(ig.ignored('file10.txt')).toBe(false);
    expect(ig.ignored('lib/x.o')).toBe(true);
    expect(ig.ignored('lib/x.c')).toBe(false);
    expect(ig.ignored('docs/draft.md')).toBe(true);
    expect(ig.ignored('docs/a/b/draft.md')).toBe(true);
    expect(ig.ignored('#notes')).toBe(true);
  });

  it('ignores nothing without a .gitignore', () => {
    expect(gitignores([['src/main.py', 'x = 1']]).ignored('node_modules/x.js')).toBe(false);
  });
});
