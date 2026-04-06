# Git Workflow

---

## Branch structure

```
main          ← always working, never commit here directly
dev           ← integration branch, merge into main when stable
feat/*        ← feature branches
fix/*         ← bug fix branches
```

---

## Starting a new feature

```bash
# Always branch from dev
git checkout dev
git pull origin dev

# Create your branch
git checkout -b feat/your-feature-name

# Work and commit
git add .
git commit -m "feat: description of what you did"

# Push it up
git push origin feat/your-feature-name
```

---

## Merging back in

1. Push your branch
2. Open a PR on GitHub: `feat/your-feature` → `dev`
3. Other person reviews and approves
4. Squash and merge
5. Delete the branch

---

## Keeping your branch up to date

If the other person merged something into `dev` while you were working:

```bash
git checkout feat/your-feature-name
git rebase dev
```

---

## Releasing to main

When `dev` is stable and tested:

```bash
git checkout main
git merge dev
git push origin main    # pushes to GitHub + Gitea in one shot
```

---

## Naming conventions

```
feat/server-udp-listener
feat/client-raycaster
feat/shared-packet-protocol
fix/player-desync
chore/update-dependencies
```

---

## The one hard rule

> Never change `shared/protocol.rs` or `shared/map.rs` alone.
> These break both binaries — discuss, agree, then PR together.
