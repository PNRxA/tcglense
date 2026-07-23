<!--
  Help shown under the catalog search boxes. The backend understands a
  Scryfall-style query grammar (see api/src/scryfall/search/); this nudges users
  toward it, and the adjacent AdvancedSearchPanel ("Filters") assembles the same
  syntax point-and-click. Unsupported operators come back as a clear error message.
-->
<template>
  <p class="text-muted-foreground text-xs">
    Scryfall-style search —
    <code class="bg-muted rounded px-1 py-0.5">t:goblin c:r</code>
    <code class="bg-muted ml-1 rounded px-1 py-0.5">mv&lt;=2</code>
    <code class="bg-muted ml-1 rounded px-1 py-0.5">o:flying r&gt;=rare</code>
    <code class="bg-muted ml-1 rounded px-1 py-0.5">art:squirrel</code>
    <a
      href="https://scryfall.com/docs/syntax"
      target="_blank"
      rel="noopener noreferrer"
      class="hover:text-foreground ml-1 underline"
    >
      syntax guide
    </a>
  </p>
</template>
