// The narrow, fully-typed surface of the OpenCV.js runtime the scanner uses. The shipped
// d.ts lags the WASM build, so the runtime is treated as untyped and only the calls the
// scan modules actually make are declared here — extend this type when a module needs a
// new call (and verify the function exists in the bundled runtime first).

export type Cv = {
  matFromImageData: (data: ImageData) => CvMat
  Mat: new () => CvMat
  MatVector: new () => CvMatVector
  Size: new (w: number, h: number) => unknown
  cvtColor: (src: CvMat, dst: CvMat, code: number) => void
  GaussianBlur: (src: CvMat, dst: CvMat, ksize: unknown, sigma: number) => void
  Canny: (src: CvMat, dst: CvMat, t1: number, t2: number) => void
  getStructuringElement: (shape: number, ksize: unknown) => CvMat
  morphologyEx: (src: CvMat, dst: CvMat, op: number, kernel: CvMat) => void
  threshold: (src: CvMat, dst: CvMat, thresh: number, maxval: number, type: number) => number
  findContours: (
    img: CvMat,
    contours: CvMatVector,
    hierarchy: CvMat,
    mode: number,
    method: number,
  ) => void
  convexHull: (src: CvMat, dst: CvMat) => void
  boundingRect: (contour: CvMat) => { x: number; y: number; width: number; height: number }
  arcLength: (curve: CvMat, closed: boolean) => number
  approxPolyDP: (curve: CvMat, approx: CvMat, epsilon: number, closed: boolean) => void
  contourArea: (contour: CvMat) => number
  COLOR_RGBA2GRAY: number
  MORPH_RECT: number
  MORPH_CLOSE: number
  THRESH_BINARY: number
  THRESH_OTSU: number
  RETR_LIST: number
  CHAIN_APPROX_SIMPLE: number
}

export interface CvMat {
  rows: number
  data: Uint8Array
  data32S: Int32Array
  delete: () => void
}

export interface CvMatVector {
  size: () => number
  get: (i: number) => CvMat
  delete: () => void
}
