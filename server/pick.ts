import { execFile } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { top } from './git.ts';

/** What a program printed and exited with, or `missing` if there is no such program. */
export type Run = (cmd: string, args: string[]) => Promise<{ code: number; out: string; err: string } | 'missing'>;

/** Runs a program (no shell) and waits for it, however long a person takes to answer. */
export const run: Run = (cmd, args) =>
  new Promise(resolve => {
    execFile(cmd, args, { encoding: 'utf8' }, (e, out, err) => {
      if (!e) resolve({ code: 0, out, err });
      else if ((e as NodeJS.ErrnoException).code === 'ENOENT') resolve('missing');
      else resolve({ code: typeof e.code === 'number' ? e.code : 1, out, err: err || e.message });
    });
  });

const TITLE = 'Choose a git repository';
const WINDOWS = `Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = '${TITLE}'; if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath } else { exit 1 }`;

/** The programs that can ask for a folder on this platform, best first, opening at `start`. */
export function dialogCommands(platform: string, start: string): [string, string[]][] {
  if (platform === 'linux') {
    return [
      ['zenity', ['--file-selection', '--directory', `--title=${TITLE}`, `--filename=${start.replace(/\/?$/, '/')}`]],
      ['kdialog', ['--getexistingdirectory', start, '--title', TITLE]],
    ];
  }
  // The folder is an argument of the script, so nothing in it is run as script.
  if (platform === 'darwin') {
    return [['osascript', ['-e', 'on run argv', '-e', `POSIX path of (choose folder with prompt "${TITLE}" default location POSIX file (item 1 of argv))`, '-e', 'end run', start]]];
  }
  if (platform === 'win32') return [['powershell', ['-NoProfile', '-Command', WINDOWS]]];
  return [];
}

/** A dialog is already open: a second would only confuse. */
export class Busy extends Error {}

/**
 * Asks for a folder in the system's own dialog and answers with the git
 * repository it is in (null: cancelled). One dialog at a time; the next
 * opens beside the repository chosen last.
 */
export function folderPicker(exec: Run = run, platform: string = process.platform, home = os.homedir()): { pick(): Promise<string | null> } {
  let start = home;
  let open = false;
  const ask = async (): Promise<string | null> => {
    for (const [cmd, args] of dialogCommands(platform, start)) {
      const r = await exec(cmd, args);
      if (r === 'missing') continue;
      // zenity and kdialog exit 1 when cancelled; osascript fails with error -128.
      if (r.code !== 0) {
        if (platform === 'darwin' ? r.err.includes('-128') : r.code === 1) return null;
        throw new Error(r.err.trim() || `${cmd} failed`);
      }
      const folder = r.out.trim();
      if (!folder) return null;
      const repo = await top(folder).catch(() => { throw new Error(`${folder} is not in a git repository`); });
      start = path.dirname(repo);
      return repo;
    }
    throw new Error('No folder dialog found (install zenity or kdialog), or type the path.');
  };
  return {
    async pick() {
      if (open) throw new Busy('A folder dialog is already open.');
      open = true;
      try { return await ask(); } finally { open = false; }
    },
  };
}
