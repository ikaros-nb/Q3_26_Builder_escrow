# Approach — q3_26_escrow (Turbin3 Q3-26)

> Reasoning log. Append to section 5, do not rewrite history. Being wrong here is expected
> and is part of what gets graded.

## 1. Understanding (before writing code)

- **What the assignment asks, in my own words:**
  _(to fill in)_

- **Inputs, outputs, and constraints I identified:**
  _(to fill in)_

- **Concepts I am unsure about:**

  - Vault account lifecycle:

    > Inside my Vault Anchor project, vault is a SystemAccount. So draining all lamports will
    > deallocate the account. But for the project here, vault is a TokenAccount which is alive
    > because of the rent-exempt amount of lamports. So after token transfer from vault to
    > taker_ata_a, we need to drain the lamports manually (by calling CloseAccount) to make that
    > happen. That is a difference between a SystemAccount and a TokenAccount. In the last one,
    > we're moving tokens, not lamports.

  - PDA signing in `take`:

    > escrow is a PDA. Seeds contain maker pubkey. No problem for refund instruction
    > (signer_seeds) but for take instruction, we're also using the signer_seeds so, will it
    > work... One maker can create as much as escrow he wants thanks to escrow.id injected into
    > the seeds. I understand that to permit the program to sign on behalf of the PDA, we need
    > the seeds, the bump and the program id to ensure no one can attack from the outside (from
    > another program). But an attacker can inject the maker address, but he will not be able to
    > sign because he doesn't own the private key of the maker. So, what about the signer_seeds
    > inside take instruction?

## 2. Design

- **Data structures / accounts / state:**
  _(to fill in — why these fields on `Escrow`, why `id` is in the seeds, why the custom
  discriminator = 1)_

- **Main steps or instruction flow:**
  _(to fill in — make / take / refund)_

- **Where I expect this to be hard, and why:**
  _(to fill in)_

## 3. Security model

- **Who can call what, and how is that enforced?**

  My current understanding of the `has_one` and `close` constraints:

  > I am not sure but maybe the has_one constraints are checking if the escrow is properly
  > initialized. And I know there will be a rejected transaction, maybe because of the owner
  > property, I don't know, and the close constraint will return the rent-exempt lamport amount
  > to the maker.

  Account ownership and lamports:

  > Indeed the runtime rule is that only an account owner program may reduce its lamports,
  > that's what I meant. So basically, the close constraints are executed after the take
  > instruction. The escrow account is owned by the program and on the other hand the vault
  > account is owned by the token program. That's the difference. So the token program can move
  > lamports from this account but it can't do that for the escrow account and vice versa.

- **What could an attacker try? What stops them?**

  **Attack found on 2026-09-04: substituted `mint_b` in `take`.**

  > If we are not checking if the escrow mint_b is a correct one inside the take account
  > structure, maybe Eve could mint a different token and pass it inside the take instruction.

  The ledger I worked out (Alice = maker, 100 RED in vault; Eve = taker, holding a mint `E` she
  created herself):

  | | Alice (maker) | Eve (taker) |
  |---|---|---|
  | Before | 100 RED in vault, 0 E | 1,000,000,000 E, 0 RED |
  | `take.rs:97` transfers ... | 100 RED in vault, 1,000,000,000 E | 0 E, 0 RED |
  | `take.rs:119` transfers ... | 0 RED in vault, 1,000,000,000 E | 0 E, 100 RED |

  > But the token E is not real, means no value. So E amount is just a number.
  >
  > Alice lose everything -> the 100 RED are loss for nothing valuable.

  _[Correction made during the session, not my original working: the `:97` row transfers
  `escrow.token_b_wanted_amount` (e.g. 50 E), not Eve's whole balance. Doesn't change the
  conclusion — Eve minted them, so any amount costs her nothing.]_

  **Why `refund` was never exposed to this:**

  > I am not handling mint_b nor ata_b nor taker accounts here. So answer is no mint_b.

  **The rule I take away:**

  > The attack surface is exactly the set of accounts an instruction accepts (the mint_b
  > constraint).

  **Fix applied:** `has_one = mint_b` on the `escrow` account in `take.rs`.

- **Trust assumptions I am making:**
  _(to fill in)_

## 4. Test plan

> So the take instruction is finished. I didn't do anything about the unit tests. I will do
> that later but I think I will create first the three logical tests, one for the make, one for
> the refund, one for the take, but I will have to think about the edge cases. And I am
> struggling figuring out what kind of edge cases actually exist.

- **Behaviors I will test:**
  - happy path: `make`
  - happy path: `refund`
  - happy path: `take`

- **Edge cases and failure paths:**

  First negative test I identified:

  > "take with a substituted mint_b must fail" is a good one.

  _(rest open — to conceive tomorrow, without AI)_

## 5. Log (append, do not rewrite)

### 2026-09-04 — state at first AI session

- **What I did:** implemented `make`, `refund`, `take`. `take` is finished as far as I can tell.
  No tests written yet (`tests/test_initialize.rs` is still the commented-out scaffold counter
  test).
- **What broke:** nothing failing yet — the open items are questions, not errors.
- **My hypothesis / open questions:**
  1. Does `invoke_signed` in `take` work even though `maker` is a `SystemAccount` and not a
     `Signer`?
  2. What do the `has_one` constraints actually check?
- **What I learned:** see the entry below.

### 2026-09-04 — after AI session

- **Questions I asked:**
  1. Vault as `SystemAccount` vs `TokenAccount` — why the token account survives a token
     transfer and needs an explicit `CloseAccount`.
  2. Whether `signer_seeds` work in `take` when `maker` is not a `Signer`.

- **What changed in my understanding:**

  > The consistency about checking not only deposit (in this context) but also other variables.
  > But the point is to keep an economically viable product for every party, and everyone could
  > make mistakes (maker input 0) so I have to think of it beforehand.

  > The attack surface is exactly the set of accounts an instruction accepts (the mint_b
  > constraint).

  Also corrected during the session (things I had wrong going in):

  - A PDA has **no private key at all**. I thought an attacker was blocked because he doesn't
    own the maker's private key — wrong. Nobody can sign for the escrow PDA with a key. The
    runtime produces the signature from `create_program_address(seeds, program_id)`, and the
    unforgeable part is **my program id**, not the maker's key. So `maker` being a
    `SystemAccount` in `take` is fine.
  - I thought `has_one` was "checking if the escrow is properly initialized" — wrong. It is a
    plain equality check: the field stored in the account data must equal the key of the account
    passed in.
  - I thought `close = maker` verified that the maker is allowed to close the account — wrong.
    `close` only names the **lamport destination**. It authorizes nothing. All authorization
    comes from `has_one` / `seeds` / signer checks.
  - The account **type** (`Account<'info, Escrow>`) only proves owner + discriminator. It proves
    the account is *an* escrow, never *whose*. Type safety is not authorization.
  - Don't pre-validate what the CPI already enforces (no `require!` on the maker's balance —
    `transfer_checked` handles it). `require!(deposit > 0)` earns its place because a 0-token
    transfer would otherwise succeed.

- **Changes I made:**
  - `take.rs` — added `has_one = mint_b` on the `escrow` account.
  - `make.rs` — added `require!(token_b_wanted_amount > 0)` for consistency with the existing
    `deposit > 0` check.

- **What I will do next, without AI:**

  > I will do the test plan tomorrow, then the tests also. So this will be my next task without
  > AI: conceiving a test plan, then implement tests.

  Method to apply when generating edge cases (four passes):
  - **Pass A** — every account in the struct: what if the caller passes a different one? Name
    the constraint that stops them. No constraint = bug or test.
  - **Pass B** — every instruction argument: 0, `u64::MAX`, values that don't match on-chain
    reality.
  - **Pass C** — ordering and repetition: called twice, raced, after close, same account twice.
  - **Pass D** — who profits: for each actor, can they end richer, or leave someone unable to
    recover funds?

### YYYY-MM-DD — next session

- **Question I asked:**
- **What changed in my understanding:**
- **What I will do next, without AI:**
