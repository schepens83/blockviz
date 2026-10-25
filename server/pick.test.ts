import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { Busy, dialogCommands, folderPicker, type Run } from './pick';

/** A runner that answers with `replies` in turn and records what it was asked. */
function fake(...replies: Awaited<ReturnType<Run>>[]) {
  const calls: [string, string[]][] = [];
  const run: Run = async (cmd, args) => { calls.push([cmd, args]); return replies.shift() ?? 'missing'; };
  return { run, calls };
}

let dir: string;
let repo: string;
beforeAll(() => {
  dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'blockviz-pick-')));
  repo = path.join(dir, 'proj');
  fs.mkdirSync(path.join(repo, 'src'), { recursive: true });
  execFileSync('git', ['init', '-q', '-b', 'main'], { cwd: repo });
});
afterAll(() => fs.rmSync(dir, { recursive: true, force: true }));

describe('dialog commands', () => {
  it('start in the given folder, without a shell', () => {
    expect(dialogCommands('linux', '/home/me')[0]).toEqual(['zenity', ['--file-selection', '--directory', '--title=Choose a git repository', '--filename=/home/me/']]);
    expect(dialogCommands('linux', '/home/me')[1][0]).toBe('kdialog');
    expect(dialogCommands('linux', '/home/me')[1][1]).toContain('/home/me');
    const [cmd, args] = dialogCommands('darwin', '/Users/me; rm -rf "')[0];
    expect(cmd).toBe('osascript');
    // The folder is an argument of the script, never part of its text.
    expect(args.slice(0, -1).join(' ')).not.toContain('rm -rf');
    expect(args[args.length - 1]).toBe('/Users/me; rm -rf "');
    expect(dialogCommands('win32', 'C:\\')[0][0]).toBe('powershell');
    expect(dialogCommands('freebsd', '/home')).toEqual([]);
  });
});

describe('the folder picker', () => {
  it('answers with the repository the chosen folder is in', async () => {
    const { run, calls } = fake({ code: 0, out: `${path.join(repo, 'src')}\n`, err: '' });
    expect(await folderPicker(run, 'linux', dir).pick()).toBe(repo);
    expect(calls).toHaveLength(1);
  });

  it('answers null when the dialog is cancelled', async () => {
    expect(await folderPicker(fake({ code: 1, out: '', err: '' }).run, 'linux', dir).pick()).toBeNull();
    expect(await folderPicker(fake({ code: 1, out: '', err: 'execution error: User canceled. (-128)' }).run, 'darwin', dir).pick()).toBeNull();
  });

  it('tries the next dialog tool when one is missing', async () => {
    const { run, calls } = fake('missing', { code: 0, out: repo, err: '' });
    expect(await folderPicker(run, 'linux', dir).pick()).toBe(repo);
    expect(calls.map(c => c[0])).toEqual(['zenity', 'kdialog']);
  });

  it('says to type the path when there is no dialog tool', async () => {
    await expect(folderPicker(fake().run, 'linux', dir).pick()).rejects.toThrow(/type the path/);
    await expect(folderPicker(fake().run, 'freebsd', dir).pick()).rejects.toThrow(/type the path/);
  });

  it('rejects a folder outside any repository, and a dialog that fails', async () => {
    await expect(folderPicker(fake({ code: 0, out: dir, err: '' }).run, 'linux', dir).pick()).rejects.toThrow(`${dir} is not in a git repository`);
    await expect(folderPicker(fake({ code: 2, out: '', err: 'no display' }).run, 'linux', dir).pick()).rejects.toThrow('no display');
  });

  it('opens one dialog at a time, and starts the next beside the last repository', async () => {
    let open!: (r: Awaited<ReturnType<Run>>) => void;
    const calls: string[][] = [];
    const run: Run = (_cmd, args) => { calls.push(args); return new Promise(res => { open = res; }); };
    const picker = folderPicker(run, 'linux', dir);
    const first = picker.pick();
    await expect(picker.pick()).rejects.toBeInstanceOf(Busy);
    open({ code: 0, out: repo, err: '' });
    await first;
    const second = picker.pick();
    expect(calls[1]).toContain(`--filename=${dir}/`);
    open({ code: 1, out: '', err: '' });
    await second;
  });
});
