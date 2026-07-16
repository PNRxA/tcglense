// One stable import factory for the sealed-product detail dialog. App.vue's lazy mount and
// ProductTile's hover warm both go through it, so import() dedupes the chunk request.
export const loadProductDetailDialog = () => import('./ProductDetailDialog.vue')
