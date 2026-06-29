import { describe, it, expect } from 'vitest'

import { ApiError } from '../api'
import { shouldRetryQuery } from '../queryClient'

describe('shouldRetryQuery', () => {
  it('never retries 4xx client errors', () => {
    expect(shouldRetryQuery(0, new ApiError('unauthorized', 401))).toBe(false)
    expect(shouldRetryQuery(0, new ApiError('unprocessable', 422))).toBe(false)
    expect(shouldRetryQuery(0, new ApiError('not found', 404))).toBe(false)
  })

  it('retries network and 5xx errors up to twice', () => {
    expect(shouldRetryQuery(0, new ApiError('server error', 500))).toBe(true)
    expect(shouldRetryQuery(1, new Error('network down'))).toBe(true)
    expect(shouldRetryQuery(2, new ApiError('unavailable', 503))).toBe(false)
  })
})
