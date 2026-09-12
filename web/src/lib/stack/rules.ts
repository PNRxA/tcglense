/**
 * The Comprehensive Rules the simulator leans on, keyed by paragraph number so a log line
 * can cite the rule it rests on and the UI can show the plain-English version in place.
 *
 * Paraphrased, not quoted: the point is the explanation a newer player needs, with the
 * paragraph number as the trail back to the authoritative text. Only the section-level
 * numbers that have stayed put across CR revisions are used (the letters under 608.2 move).
 */
export interface Rule {
  id: string
  title: string
  text: string
}

export const RULES: Readonly<Record<string, Rule>> = {
  '117.1': {
    id: '117.1',
    title: 'You need priority to act',
    text:
      'A player may cast a spell or activate an ability only when they have priority. An ' +
      'instant (or a card with flash) and an ability can be used whenever you have priority; ' +
      'a sorcery, creature, artifact or enchantment only during your own main phase while ' +
      'the stack is empty.',
  },
  '117.3b': {
    id: '117.3b',
    title: 'The active player gets priority after a resolution',
    text:
      'After a spell or ability resolves, the active player (whose turn it is) receives ' +
      'priority — not the player who cast the spell that just resolved.',
  },
  '117.3c': {
    id: '117.3c',
    title: 'You keep priority after casting',
    text:
      'When you cast a spell or activate an ability, you receive priority again afterwards. ' +
      'You may act again immediately — "holding priority" — before your opponent gets a chance ' +
      'to respond. Usually you pass, and the turn order carries priority to your opponent.',
  },
  '117.3d': {
    id: '117.3d',
    title: 'Passing hands priority to the next player',
    text:
      'When a player with priority chooses not to act, they pass. The next player in turn ' +
      'order then receives priority.',
  },
  '117.4': {
    id: '117.4',
    title: 'All players pass → the top of the stack resolves',
    text:
      'If all players pass in succession — without anyone taking an action in between — the ' +
      'top object on the stack resolves. If the stack is empty instead, the current phase or ' +
      'step ends. Any action in between (casting, activating) resets the count: everyone ' +
      'must pass again.',
  },
  '117.5': {
    id: '117.5',
    title: 'Before anyone gets priority: state-based actions, then triggers',
    text:
      'Each time a player would receive priority, the game first performs state-based actions ' +
      '(creatures with lethal damage die, a player at 0 life loses), then puts any waiting ' +
      'triggered abilities on the stack. Only then does the player actually get priority.',
  },
  '405.1': {
    id: '405.1',
    title: 'The stack is last in, first out',
    text:
      'Spells and abilities go on top of the stack when cast or activated and wait there. ' +
      'The most recently added object is the first to resolve, so a response always resolves ' +
      'before the thing it responds to.',
  },
  '500.2': {
    id: '500.2',
    title: 'A phase ends when everyone passes on an empty stack',
    text:
      'A phase or step in which players receive priority ends when the stack is empty and all ' +
      'players pass in succession. The simulator then skips ahead to the other player’s main ' +
      'phase.',
  },
  '514.2': {
    id: '514.2',
    title: 'Cleanup: damage and "until end of turn" effects go away',
    text:
      'During the cleanup step at the end of each turn, all damage marked on permanents is ' +
      'removed and every "until end of turn" effect ends at the same time.',
  },
  '601.2': {
    id: '601.2',
    title: 'Casting a spell puts it on the stack',
    text:
      'To cast a spell, the card is moved onto the top of the stack, its targets are chosen, ' +
      'and its cost is paid. It then waits there until it resolves or is countered — it does ' +
      'nothing yet.',
  },
  '602.2': {
    id: '602.2',
    title: 'Activated abilities use the stack too',
    text:
      'Activating an ability puts it on the stack just like a spell; it can be responded to ' +
      'and countered by effects that counter abilities. The ability is independent of its ' +
      'source once activated: removing the permanent does not remove the ability.',
  },
  '603.2': {
    id: '603.2',
    title: 'Triggered abilities trigger on an event',
    text:
      'A triggered ability ("whenever", "when", "at") triggers automatically the moment its ' +
      'event happens. It does not go on the stack right away — it waits for the next time a ' +
      'player would receive priority.',
  },
  '603.3': {
    id: '603.3',
    title: 'Triggers are put on the stack before priority',
    text:
      'The next time a player would receive priority, each waiting triggered ability is put on ' +
      'the stack. A trigger from casting a spell therefore sits above the spell — and resolves ' +
      'before it.',
  },
  '603.3b': {
    id: '603.3b',
    title: 'APNAP: the active player’s triggers go on first',
    text:
      'When several abilities trigger at once, the active player puts all of theirs on the ' +
      'stack first, then each other player in turn order. Because the stack is last in, first ' +
      'out, the non-active player’s triggers resolve first.',
  },
  '605.3': {
    id: '605.3',
    title: 'Mana abilities skip the stack',
    text:
      'A mana ability resolves immediately, without using the stack. It cannot be targeted, ' +
      'countered or responded to — which is why you can always tap for mana, even while a ' +
      'split-second spell is on the stack.',
  },
  '608.2b': {
    id: '608.2b',
    title: 'No legal targets → it doesn’t resolve ("fizzles")',
    text:
      'As a spell or ability starts to resolve, it checks its targets again. If every target ' +
      'has become illegal — it left the zone it was in, or no longer matches what the spell ' +
      'can target — the spell doesn’t resolve at all and none of its effects happen. An ' +
      'instant or sorcery is simply put into the graveyard.',
  },
  '608.2': {
    id: '608.2',
    title: 'An instant or sorcery resolves, then goes to the graveyard',
    text:
      'When an instant or sorcery resolves, its instructions are followed in order; then the ' +
      'card is put into its owner’s graveyard as the final step.',
  },
  '608.3': {
    id: '608.3',
    title: 'A resolving permanent spell enters the battlefield',
    text:
      'When a creature, artifact or enchantment spell resolves, it becomes a permanent and is ' +
      'put onto the battlefield under its controller’s control. "When this enters" abilities ' +
      'trigger at that moment.',
  },
  '701.5': {
    id: '701.5',
    title: 'Countering removes a spell from the stack',
    text:
      'To counter a spell or ability is to remove it from the stack so it never resolves. A ' +
      'countered spell goes to its owner’s graveyard; a countered ability simply ceases to ' +
      'exist. A countered permanent spell never enters the battlefield.',
  },
  '702.8': {
    id: '702.8',
    title: 'Flash',
    text:
      'A card with flash may be cast any time you could cast an instant — including in ' +
      'response to another spell, or on your opponent’s turn.',
  },
  '702.61': {
    id: '702.61',
    title: 'Split second',
    text:
      'While a spell with split second is on the stack, players can’t cast spells or activate ' +
      'abilities that aren’t mana abilities. Triggered abilities still trigger and still go on ' +
      'the stack, and mana abilities still work.',
  },
  '704.3': {
    id: '704.3',
    title: 'State-based actions are checked before priority',
    text:
      'Whenever a player would receive priority, the game checks for state-based actions: a ' +
      'creature with lethal damage or 0 toughness is put into the graveyard, a player at 0 or ' +
      'less life loses. They happen automatically — nobody controls them, and they can’t be ' +
      'responded to.',
  },
  '704.5a': {
    id: '704.5a',
    title: 'A player at 0 life loses',
    text: 'A player with 0 or less life loses the game the next time state-based actions are checked.',
  },
  '704.5f': {
    id: '704.5f',
    title: 'A creature with 0 toughness dies',
    text:
      'A creature with toughness 0 or less is put into its owner’s graveyard — this is not ' +
      'destruction, so it can’t be regenerated.',
  },
  '704.5g': {
    id: '704.5g',
    title: 'Lethal damage destroys a creature',
    text:
      'A creature with damage marked on it equal to or greater than its toughness is destroyed. ' +
      'Damage stays marked until the cleanup step, so two small burn spells add up.',
  },
  '707.10': {
    id: '707.10',
    title: 'A copy of a spell exists only on the stack',
    text:
      'Copying a spell puts a copy of it directly onto the stack (the copy is not "cast"). It ' +
      'has the same targets unless the copying effect lets you choose new ones, resolves like ' +
      'the original, and then ceases to exist rather than going to a graveyard.',
  },
}

export function ruleFor(id: string | undefined): Rule | undefined {
  return id ? RULES[id] : undefined
}
