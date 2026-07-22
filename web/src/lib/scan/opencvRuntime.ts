// Local ESM wrapper around the CJS `@techstark/opencv-js` package, existing solely to
// defeat a production-only bundler-interop trap (see loadOpenCv in opencvDetect.ts).
//
// The package's `default` export is a real `Promise` that resolves to the initialised
// `cv` runtime. When the app dynamically `import()`s the package directly, Rolldown's
// generated CJS→ESM interop (`__toESM`) re-wraps that promise in an object that inherits
// `Promise.prototype` (so `x instanceof Promise` is true) but carries no internal promise
// state. Rolldown returns that fake-thenable from the dynamic import's own `.then`, so the
// runtime tries to assimilate it, calls `Promise.prototype.then` on an incompatible
// receiver, and throws — before any of our code runs. Detection then silently fell back to
// the basic detector in production while working in dev (whose optimizer hands the module
// over in a different shape).
//
// Importing the package with a *static* default import here keeps the interop local: this
// module re-exports the genuine promise, and because this wrapper is itself a real ES
// module, dynamically importing *it* yields a normal namespace whose `default` is the real
// promise — which flows through `.then` correctly in dev and prod alike. opencv stays in a
// lazily-loaded chunk (this wrapper is only ever reached via dynamic import).
import cv from '@techstark/opencv-js'

export default cv
