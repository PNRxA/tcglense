// Single shared import factory for the card-detail dialog: every call site (App.vue's
// lazy mount, CardTile's hover warm) goes through this one function so they all hit the
// SAME deduped chunk fetch instead of minting separate chunks.
export const loadCardDetailDialog = () => import('./CardDetailDialog.vue')
