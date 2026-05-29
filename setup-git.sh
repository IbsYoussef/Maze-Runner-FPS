#!/bin/bash
# Sets up dual-push remotes for GitHub + Gitea
# Run once after cloning: bash setup-git.sh

echo "Setting up git remotes..."

# Add Gitea as its own named remote if it doesn't exist
if ! git remote | grep -q "^gitea$"; then
    git remote add gitea https://learn.01founders.co/git/iyoussef/Multiplayer-FPS.git
    echo "Added gitea remote"
else
    echo "gitea remote already exists, skipping"
fi

# Reset origin push URLs to avoid duplicates
git remote set-url --push origin https://github.com/IbsYoussef/Maze-Runner-FPS.git
git remote set-url --add --push origin https://learn.01founders.co/git/iyoussef/Multiplayer-FPS.git

echo ""
echo "Done! Verifying remotes:"
git remote -v
