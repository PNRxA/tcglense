// OpenCV.js card detection: find the card's four corners in a camera frame robustly
// (Canny edges → contours → largest card-shaped quadrilateral), for the live outline and
// a tight, deskewed capture crop. OpenCV.js is a ~13 MB WASM payload, so it's lazily
// imported the first time the scanner starts (never at app load), like tesseract.
//
// Corners are returned NORMALISED (0..1 of the frame) so the same quad drives both the
// on-screen outline (any display size) and the capture warp (any capture resolution).
// The lightweight `detect.ts` detector stays as the fallback when OpenCV isn't loaded.

import { orderCorners, type Point, type Quad } from './detect'
import { CARD_ASPECT } from './regions'

// The OpenCV runtime is untyped-ish (the shipped d.ts lags the build), so `cv` is `any`;
// the exported surface below is fully typed.
type Cv = {
  matFromImageData: (data: ImageData) => CvMat
  Mat: new () => CvMat
  MatVector: new () => CvMatVector
  Size: new (w: number, h: number) => unknown
  cvtColor: (src: CvMat, dst: CvMat, code: number) => void
  GaussianBlur: (src: CvMat, dst: CvMat, ksize: unknown, sigma: number) => void
  Canny: (src: CvMat, dst: CvMat, t1: number, t2: number) => void
  getStructuringElement: (shape: number, ksize: unknown) => CvMat
  dilate: (src: CvMat, dst: CvMat, kernel: CvMat) => void
  findContours: (
    img: CvMat,
    contours: CvMatVector,
    hierarchy: CvMat,
    mode: number,
    method: number,
  ) => void
  arcLength: (curve: CvMat, closed: boolean) => number
  approxPolyDP: (curve: CvMat, approx: CvMat, epsilon: number, closed: boolean) => void
  isContourConvex: (contour: CvMat) => boolean
  contourArea: (contour: CvMat) => number
  COLOR_RGBA2GRAY: number
  MORPH_RECT: number
  RETR_LIST: number
  CHAIN_APPROX_SIMPLE: number
}
interface CvMat {
  rows: number
  data32S: Int32Array
  delete: () => void
}
interface CvMatVector {
  size: () => number
  get: (i: number) => CvMat
  delete: () => void
}

let cvPromise: Promise<Cv> | null = null

/** Lazily load + initialise the OpenCV.js runtime (cached). Rejects (and clears the
 * cache so a later retry can succeed) if the payload fails to load. */
export function loadOpenCv(): Promise<Cv> {
  if (!cvPromise) {
    cvPromise = import('@techstark/opencv-js')
      .then(async (mod): Promise<Cv> => {
        const cvModule = (mod.default ?? mod) as unknown as Cv & {
          onRuntimeInitialized?: () => void
        }
        if (cvModule instanceof Promise) return (await cvModule) as Cv
        if (cvModule.Mat) return cvModule
        await new Promise<void>((resolve) => {
          cvModule.onRuntimeInitialized = () => resolve()
        })
        return cvModule
      })
      .catch((err) => {
        cvPromise = null
        throw err
      })
  }
  return cvPromise
}

/** Relative tolerance on the card aspect ratio (perspective foreshortens it). */
const ASPECT_TOLERANCE = 0.28

/** Whether a normalised quad is card-shaped: aspect near 61:85 and roughly rectangular
 * (opposite sides similar), so text boxes / hands / random rectangles are rejected. */
function isCardQuad(quad: Quad, frameAspect: number): boolean {
  const [tl, tr, br, bl] = quad
  const dist = (a: Point, b: Point) => Math.hypot((a.x - b.x) * frameAspect, a.y - b.y)
  const top = dist(tl, tr)
  const bottom = dist(bl, br)
  const left = dist(tl, bl)
  const right = dist(tr, br)
  if (Math.min(top, bottom, left, right) <= 0) return false
  if (Math.max(top, bottom) / Math.min(top, bottom) > 1.4) return false
  if (Math.max(left, right) / Math.min(left, right) > 1.4) return false
  const aspect = (top + bottom) / 2 / ((left + right) / 2)
  return Math.abs(aspect - CARD_ASPECT) <= ASPECT_TOLERANCE
}

/** Detect the largest card-shaped quadrilateral in `imageData`, or null. Corners are
 * returned normalised (0..1) and ordered [TL, TR, BR, BL]. All OpenCV mats are freed. */
export function detectCardQuadCv(cv: Cv, imageData: ImageData): Quad | null {
  const w = imageData.width
  const h = imageData.height
  const frameAspect = w / h
  const src = cv.matFromImageData(imageData)
  const gray = new cv.Mat()
  const blur = new cv.Mat()
  const edges = new cv.Mat()
  const contours = new cv.MatVector()
  const hierarchy = new cv.Mat()
  try {
    cv.cvtColor(src, gray, cv.COLOR_RGBA2GRAY)
    cv.GaussianBlur(gray, blur, new cv.Size(5, 5), 0)
    cv.Canny(blur, edges, 60, 160)
    // Close small gaps in the card outline so it forms one closed contour.
    const kernel = cv.getStructuringElement(cv.MORPH_RECT, new cv.Size(3, 3))
    cv.dilate(edges, edges, kernel)
    kernel.delete()
    cv.findContours(edges, contours, hierarchy, cv.RETR_LIST, cv.CHAIN_APPROX_SIMPLE)

    const minArea = w * h * 0.1
    let best: Quad | null = null
    let bestArea = 0
    for (let i = 0; i < contours.size(); i++) {
      const cnt = contours.get(i)
      const approx = new cv.Mat()
      cv.approxPolyDP(cnt, approx, 0.02 * cv.arcLength(cnt, true), true)
      if (approx.rows === 4 && cv.isContourConvex(approx)) {
        const area = cv.contourArea(approx)
        if (area >= minArea && area > bestArea) {
          const pts: Point[] = []
          for (let j = 0; j < 4; j++) {
            pts.push({ x: approx.data32S[j * 2]! / w, y: approx.data32S[j * 2 + 1]! / h })
          }
          const quad = orderCorners(pts)
          if (isCardQuad(quad, frameAspect)) {
            best = quad
            bestArea = area
          }
        }
      }
      approx.delete()
      cnt.delete()
    }
    return best
  } finally {
    src.delete()
    gray.delete()
    blur.delete()
    edges.delete()
    contours.delete()
    hierarchy.delete()
  }
}
