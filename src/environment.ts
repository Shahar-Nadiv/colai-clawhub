// The environment the toolbar is given, which is not always the one it inherits.
//
// OpenClaw's Gateway passes its own environment to everything it spawns, and on Ubuntu
// that environment is often not a plain desktop one. Snap-confined applications — VS Code,
// terminals, browsers, all commonly installed as snaps — rewrite the variables that say
// where GTK modules, GIO modules, locales and libraries live, pointing them inside the
// snap. Anything they launch inherits that, including a Gateway started from a snap's
// integrated terminal.
//
// The toolbar is a system GTK program. Handed a snap's module paths it loads that snap's
// libraries against the system libc and dies before it draws anything:
//
//     symbol lookup error: /snap/core20/current/lib/x86_64-linux-gnu/libpthread.so.0:
//     undefined symbol: __libc_pthread_init, version GLIBC_PRIVATE
//
// — exit 127, no window, and a message that tells the person nothing they can act on. It
// is not a packaging fault and no dependency is missing: the same binary starts normally
// the moment those variables are gone.
//
// So they are dropped, by value rather than by name. A variable is only removed when what
// it points at is inside a snap, which leaves alone anybody who has set these deliberately
// and does not depend on knowing which snap did the rewriting.

/**
 * Variables that redirect where code, modules or locales are loaded from.
 *
 * Only these. `XDG_DATA_DIRS` and friends also get rewritten and are deliberately left:
 * they decide which icons and themes are found, so a snap's copy is cosmetically wrong
 * and never fatal, and clearing them would take the desktop's real theme with it.
 */
const LOADED_FROM = [
  "LD_LIBRARY_PATH",
  "LD_PRELOAD",
  "GTK_PATH",
  "GTK_EXE_PREFIX",
  "GTK_IM_MODULE_FILE",
  "GIO_MODULE_DIR",
  "GSETTINGS_SCHEMA_DIR",
  "LOCPATH",
];

/** Whether a value sends the loader inside a snap. */
function intoASnap(value: string | undefined): boolean {
  return value !== undefined && /(^|:)\/snap\//.test(value);
}

/**
 * The environment to start the toolbar in, given the one this process has.
 *
 * Returns a copy with the snap-confined loader paths removed. Everything else is passed
 * through untouched — the toolbar reads `DISPLAY`, `XAUTHORITY`, `HOME` and the Gateway's
 * own variables out of it.
 */
export function withoutSomebodyElsesLibraries(
  inherited: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const given: NodeJS.ProcessEnv = { ...inherited };
  for (const name of LOADED_FROM) {
    if (intoASnap(given[name])) {
      delete given[name];
    }
  }
  return given;
}
