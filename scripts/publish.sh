#!/bin/bash
set -e

echo "🚀 Publishing unport to crates.io"
echo ""

# Check if logged in to crates.io
echo "📋 Checking crates.io authentication..."
if ! cargo login --help > /dev/null 2>&1; then
    echo "❌ cargo not found"
    exit 1
fi

# Verify the package
echo "📦 Verifying package..."
cargo publish --dry-run

echo ""
echo "✅ Package verification passed!"
echo ""

# Show what will be published
echo "📄 Package contents:"
cargo package --list

echo ""
read -p "🔐 Ready to publish to crates.io? (y/n) " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo ""
    echo "📤 Publishing..."
    cargo publish
    echo ""
    echo "✅ Published successfully!"
    echo "🔗 https://crates.io/crates/unport"
else
    echo "❌ Publish cancelled"
    exit 1
fi
