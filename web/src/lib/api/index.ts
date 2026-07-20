// Public surface of the API client. Split into `client` (fetch wrapper + ApiError),
// `auth` (session endpoints) and `catalog` (game/set/card reads); this barrel keeps
// the single `@/lib/api` entrypoint every importer already uses. `request`,
// `RequestOptions` and `API_URL` stay module-private (not re-exported here).
export { ApiError } from './client'
export * from './auth'
export * from './config'
export * from './openapi'
export * from './currency'
export * from './catalog'
export * from './scan'
export * from './products'
export * from './product-holdings'
export * from './collection'
export * from './collection-import'
export * from './publicCollection'
export * from './publicWishlist'
export * from './wishlist'
export * from './decks'
export * from './alerts'
export * from './api-keys'
