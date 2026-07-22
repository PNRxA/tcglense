import { describe, expect, it } from 'vitest'
import type { Quad } from '../detect'
import {
  cropImageData,
  cropQuadToFull,
  fullFrameWindow,
  physicalSides,
  priorSearchWindow,
} from '../guidedDetect'

function quadAt(x: number, y: number, w: number, h: number): Quad {
  return [
    { x, y },
    { x: x + w, y },
    { x: x + w, y: y + h },
    { x, y: y + h },
  ]
}

describe('fullFrameWindow / physicalSides', () => {
  it('treats every side of the full frame as physical', () => {
    expect(physicalSides(fullFrameWindow(640, 480))).toEqual({
      left: true,
      top: true,
      right: true,
      bottom: true,
    })
  })

  it('marks only frame-coincident crop sides as physical', () => {
    const window = { x: 0, y: 40, width: 300, height: 440, fullWidth: 640, fullHeight: 480 }
    expect(physicalSides(window)).toEqual({
      left: true,
      top: false,
      right: false,
      bottom: true,
    })
  })
})

describe('priorSearchWindow', () => {
  it('pads the prior bounds and stays inside the frame', () => {
    const window = priorSearchWindow(quadAt(0.3, 0.2, 0.4, 0.6), 640, 480)
    // Prior bounds: x 192..448, y 96..384. Pad: max(12, 51.2, 46.08) → 51.2 px on x,
    // max(12, 38.4, 51.84) → 51.84 px on y.
    expect(window.x).toBe(Math.floor(192 - 51.2))
    expect(window.y).toBe(Math.floor(96 - 51.84))
    expect(window.x + window.width).toBe(Math.ceil(448 + 51.2))
    expect(window.y + window.height).toBe(Math.ceil(384 + 51.84))
    expect(window.fullWidth).toBe(640)
    expect(window.fullHeight).toBe(480)
  })

  it('clamps to the frame for a prior near an edge (sides become physical)', () => {
    const window = priorSearchWindow(quadAt(0.01, 0.01, 0.5, 0.7), 640, 480)
    expect(window.x).toBe(0)
    expect(window.y).toBe(0)
    expect(physicalSides(window).left).toBe(true)
    expect(physicalSides(window).top).toBe(true)
    expect(physicalSides(window).right).toBe(false)
    expect(physicalSides(window).bottom).toBe(false)
  })
})

describe('cropQuadToFull', () => {
  it('round-trips a quad through a crop exactly', () => {
    const window = { x: 100, y: 60, width: 300, height: 320, fullWidth: 640, fullHeight: 480 }
    // A quad detected at crop pixels (10,20)..(290,300).
    const cropQuad: Quad = [
      { x: 10, y: 20 },
      { x: 290, y: 20 },
      { x: 290, y: 300 },
      { x: 10, y: 300 },
    ]
    const full = cropQuadToFull(cropQuad, window)
    expect(full[0]).toEqual({ x: 110 / 640, y: 80 / 480 })
    expect(full[2]).toEqual({ x: 390 / 640, y: 360 / 480 })
  })
})

describe('cropImageData', () => {
  it('copies exactly the window pixels', () => {
    const width = 8
    const height = 6
    const data = new Uint8ClampedArray(width * height * 4)
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        data[(y * width + x) * 4] = y * width + x // unique red channel per pixel
        data[(y * width + x) * 4 + 3] = 255
      }
    }
    const image = { data, width, height } as unknown as ImageData
    const crop = cropImageData(image, {
      x: 2,
      y: 1,
      width: 3,
      height: 4,
      fullWidth: width,
      fullHeight: height,
    })
    expect(crop.width).toBe(3)
    expect(crop.height).toBe(4)
    // Top-left of the crop is full-frame pixel (2, 1); bottom-right is (4, 4).
    expect(crop.data[0]).toBe(1 * width + 2)
    expect(crop.data[(3 * 3 + 2) * 4]).toBe(4 * width + 4)
  })
})
