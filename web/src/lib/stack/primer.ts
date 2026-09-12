/**
 * The "how the stack works" reading order: which rules, grouped how, the explainer below the
 * simulator walks through. The text itself lives in `rules.ts` so the primer, the log chips
 * and the hint panel can never disagree about what a rule says.
 */
export interface PrimerSection {
  title: string
  intro: string
  rules: string[]
}

export const PRIMER: readonly PrimerSection[] = [
  {
    title: 'Priority: whose turn is it to act?',
    intro:
      'Magic never lets both players act at once. At any moment exactly one player has ' +
      '"priority" — the right to cast a spell, activate an ability, or pass.',
    rules: ['117.1', '117.3c', '117.3d', '117.4', '117.3b'],
  },
  {
    title: 'The stack: last in, first out',
    intro:
      'Spells and abilities don’t happen when they’re cast. They wait on the stack, and the ' +
      'most recent one resolves first — that is what makes a "response" work.',
    rules: ['601.2', '602.2', '405.1', '608.2', '608.3'],
  },
  {
    title: 'What can be cast when',
    intro:
      'Only instants, cards with flash and abilities can be used in response. Everything ' +
      'else needs an empty stack on your own main phase.',
    rules: ['702.8', '702.61', '605.3'],
  },
  {
    title: 'Triggered abilities',
    intro:
      '"Whenever" and "when" abilities trigger on their event, then wait for the next time a ' +
      'player would get priority before joining the stack.',
    rules: ['603.2', '603.3', '603.3b'],
  },
  {
    title: 'Before anyone gets priority',
    intro:
      'The game tidies up between actions: state-based actions first, then waiting ' +
      'triggers — and only then does a player receive priority.',
    rules: ['117.5', '704.3', '704.5g', '704.5f', '704.5a'],
  },
  {
    title: 'Countering, fizzling and copying',
    intro: 'Three ways a spell can end other than by resolving normally.',
    rules: ['701.5', '608.2b', '707.10'],
  },
  {
    title: 'The end of a phase',
    intro: 'When there is nothing left to do, the game moves on.',
    rules: ['500.2', '514.2'],
  },
]
