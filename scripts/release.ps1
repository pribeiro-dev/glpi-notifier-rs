param(
  [Parameter(Mandatory=$true)]
  [string]$Version,
  [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

function Write-Info($msg){ Write-Host "[INFO] $msg" -ForegroundColor Cyan }
function Write-Warn($msg){ Write-Warning $msg }
function Die($msg){ Write-Error $msg; exit 1 }

# Normalize version (strip leading v for Cargo.toml)
$semver = $Version.Trim()
if ($semver.StartsWith("v")) { $semver = $semver.Substring(1) }

if ($semver -notmatch '^\d+\.\d+\.\d+$') {
  Die "Version must be semantic (e.g., 0.1.2 or v0.1.2)"
}

$tag = "v$semver"

# Ensure we're at repo root (has Cargo.toml)
$root = Split-Path -Parent $MyInvocation.MyCommand.Path | Split-Path -Parent
$CargoPath = Join-Path $root "Cargo.toml"
if (-not (Test-Path $CargoPath)) {
  Die "Run this script from repo (scripts\release.ps1). Cargo.toml not found."
}

# Update Cargo.toml package.version
Write-Info "Updating Cargo.toml to version $semver"
$toml = Get-Content $CargoPath -Raw -Encoding UTF8
$toml = $toml -replace '(?ms)(^\s*version\s*=\s*")(\d+\.\d+\.\d+)(")', "`${1}$semver`${3}"
Set-Content -Path $CargoPath -Value $toml -NoNewline -Encoding UTF8

# Optionally update CHANGELOG.md (insert empty section for this version with today's date, keeping [Unreleased])
$Changelog = Join-Path $root "CHANGELOG.md"
$today = (Get-Date).ToString("yyyy-MM-dd")
if (Test-Path $Changelog) {
  Write-Info "Touching CHANGELOG.md"
  $cl = Get-Content $Changelog -Raw -Encoding UTF8

  if ($cl -notmatch [regex]::Escape("## [$semver]")) {
    $cl = $cl -replace '## \[Unreleased\](\r?\n)+', ("## [Unreleased]`r`n`r`n## [$semver] - $today`r`n`r`n")
    Set-Content -Path $Changelog -Value $cl -NoNewline -Encoding UTF8
  } else {
    Write-Warn "CHANGELOG already has entry for $semver; leaving as is."
  }
} else {
  Write-Warn "CHANGELOG.md not found; skipping."
}

# Optional: build to ensure it compiles before tagging
if (-not $NoBuild) {
  Write-Info "Building (cargo build --release)..."
  Push-Location $root
  try {
    cargo build --release
  } finally {
    Pop-Location
  }
}

# Git commit and tag
Push-Location $root
try {
  git add Cargo.toml CHANGELOG.md 2>$null
  $status = git status --porcelain
  if ($status) {
    Write-Info "Committing changes"
    git commit -m "chore(release): $tag"
  } else {
    Write-Info "No changes to commit"
  }

  # Create or move lightweight tag
  $existing = git tag --list $tag
  if ($existing) {
    Write-Warn "Tag $tag already exists; will delete and recreate."
    git tag -d $tag | Out-Null
  }
  git tag $tag -m "Release $tag"
  Write-Info "Pushing to origin (main + tag $tag)"
  git push origin main
  git push origin $tag
} finally {
  Pop-Location
}
Write-Info "Done. GitHub Action 'Release (Windows)' should start for $tag."