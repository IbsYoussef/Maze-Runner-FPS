# Git Remote Setup

This project pushes to two remotes simultaneously — GitHub (collaboration) and Gitea (course submission).

## Remotes

| Remote   | URL                                                            | Purpose             |
| -------- | -------------------------------------------------------------- | ------------------- |
| `origin` | `https://github.com/IbsYoussef/Maze-Runner-FPS.git`            | Collaboration & PRs |
| `gitea`  | `https://learn.01founders.co/git/iyoussef/Multiplayer-FPS.git` | Course submission   |

## One-time setup (run after cloning)

A setup script is included in the repo root. Run it once after cloning:

```bash
bash setup-git.sh
```

This configures both remotes automatically and is safe to re-run.

> ⚠️ Do not run `git remote add origin <gitea-url>` — `origin` already exists. Gitea must be added as its own separate remote.

**Expected output:**

```
gitea   https://learn.01founders.co/git/iyoussef/Multiplayer-FPS.git (fetch)
gitea   https://learn.01founders.co/git/iyoussef/Multiplayer-FPS.git (push)
origin  https://github.com/IbsYoussef/Maze-Runner-FPS.git (fetch)
origin  https://github.com/IbsYoussef/Maze-Runner-FPS.git (push)
origin  https://learn.01founders.co/git/iyoussef/Multiplayer-FPS.git (push)
```

## Daily workflow

```bash
git push origin main   # pushes to GitHub + Gitea in one command
git pull origin main   # always pulls from GitHub
```
