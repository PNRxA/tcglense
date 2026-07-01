<template>
  <!-- The search/filter bar that sticks to the top of the viewport so it stays reachable
       while scrolling a long list. Shared by the catalog browse/set/game views and the
       collection browse view; the slot holds the search box (and, on the game landing, a
       "View all cards" button). Extra classes passed by the caller merge onto the root via
       Vue's class fallthrough — e.g. the game view adds `mb-6 flex items-center gap-3`. -->
  <div class="bg-background/85 sticky top-0 z-30 -mx-4 border-b px-4 py-3 backdrop-blur">
    <slot />
  </div>
</template>
