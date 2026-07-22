// @vitest-environment node
//
// Guards the production-only OpenCV bundler-interop fix (see opencvRuntime.ts): the app must
// import `@techstark/opencv-js` through exactly one local ESM wrapper, and `loadOpenCv` must go
// through that wrapper. `@techstark/opencv-js`'s `default` export is a real Promise; a *direct*
// dynamic `import('@techstark/opencv-js')` lets Rolldown's `__toESM` interop re-wrap it into a
// fake-thenable that throws `Promise.prototype.then called on incompatible receiver` in the
// production build — which silently dropped the scanner to basic detection. The wrapper keeps
// that interop local so the real promise flows through. These are structural assertions because
// the failure only manifests in a bundled browser build, which a Node unit test cannot reproduce.

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const scanDir = fileURLToPath(new URL('..', import.meta.url))
const read = (file: string) => readFileSync(`${scanDir}${file}`, 'utf8')

describe('opencv runtime import boundary', () => {
  it('imports the opencv package only through the ESM wrapper', () => {
    // Any app module other than the wrapper importing '@techstark/opencv-js' directly would
    // reintroduce the interop trap. Tests may still require() the package for the Node runtime.
    const runtime = read('opencvRuntime.ts')
    expect(runtime).toMatch(/from '@techstark\/opencv-js'/)

    const detect = read('opencvDetect.ts')
    expect(detect).not.toMatch(/import\((['"`])@techstark\/opencv-js\1\)/)
    expect(detect).toMatch(/import\((['"`])\.\/opencvRuntime\1\)/)
  })

  it('re-exports the package default unchanged so the genuine promise survives', () => {
    // The wrapper must forward the package's own default export (the real promise). Anything
    // that reconstructs or re-wraps it would recreate the fake-thenable this fix removes.
    const runtime = read('opencvRuntime.ts')
    expect(runtime).toMatch(/import cv from '@techstark\/opencv-js'/)
    expect(runtime).toMatch(/export default cv/)
  })
})
