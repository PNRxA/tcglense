// Trigger a browser file download of an in-memory blob. Used for CSV exports fetched
// with an auth header (which can't be a plain `<a download href>` — the request needs the
// bearer token), so we fetch the bytes, then hand the browser an object URL to save.

/** Save `blob` to the user's downloads as `filename` via a transient object URL. */
export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = filename
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  URL.revokeObjectURL(url)
}
