# cgc.spec
# Multi-platform PyInstaller build spec for CodeGraphContext
# Supports: Linux (x86_64/Aarch64), Windows, macOS

import sys
import os
from pathlib import Path
from PyInstaller.utils.hooks import collect_data_files, collect_submodules

block_cipher = None

# ── Environment Detection ──────────────────────────────────────────────────
is_win = sys.platform == 'win32'
is_mac = sys.platform == 'darwin'
is_linux = sys.platform == 'linux' or sys.platform == 'linux2'

# Find site-packages dynamically
# If we are in the venv, we can use sys.prefix
prefix = Path(sys.prefix)
if is_win:
    site_packages = prefix / 'Lib' / 'site-packages'
else:
    # On Linux/Mac, find the pythonX.Y directory
    lib_dir = prefix / 'lib'
    py_dir = next(lib_dir.glob('python3.*'))
    site_packages = py_dir / 'site-packages'

print(f"Detected Platform: {sys.platform}")
print(f"Using site-packages: {site_packages}")

# ── 1. Binary files to bundle (.so, .pyd, .dylib) ───────────────────────────
binaries = []

# Bin extensions by platform
ext = '*.so'
if is_win:
    ext = '*.pyd'
elif is_mac:
    ext = '*.dylib'

def add_binary(package_path, pattern, target_subdir=None):
    pkg_dir = site_packages / package_path
    if pkg_dir.exists():
        for f in pkg_dir.glob(pattern):
            binaries.append((str(f), target_subdir or package_path))

# tree-sitter core
add_binary('tree_sitter', ext)

# tree-sitter-language-pack: ALL language bindings
add_binary('tree_sitter_language_pack/bindings', ext)

# other tree-sitter bindings
add_binary('tree_sitter_yaml', ext)
add_binary('tree_sitter_embedded_template', ext)
add_binary('tree_sitter_c_sharp', ext)

# KùzuDB native extension
add_binary('kuzu', ext)

# FalkorDB Lite (only for Unix-like systems)
if not is_win:
    add_binary('redislite/bin', '*')
    add_binary('falkordblite.scripts', ext)
    
    # Also include the python modules as datas if they aren't being picked up
    def add_package_data(package_name):
        pkg_path = site_packages / package_name
        if pkg_path.exists():
            datas.append((str(pkg_path), package_name))
            
    add_package_data('redislite')
    add_package_data('falkordblite')

# ── 2. Data files ────────────────────────────────────────────────────────────
datas = []

# stdlibs: dynamically imports py3.py, py312.py, etc. via importlib
stdlibs_dir = site_packages / 'stdlibs'
if stdlibs_dir.exists():
    for f in stdlibs_dir.glob('*.py'):
        datas.append((str(f), 'stdlibs'))

# mcp package data
datas += collect_data_files('mcp', includes=['**/*'])

# mcp.json shipped with CGC
mcp_json = Path('src/codegraphcontext/mcp.json')
if mcp_json.exists():
    datas.append((str(mcp_json), 'codegraphcontext'))

# tree-sitter-language-pack metadata
ts_pack_dir = site_packages / 'tree_sitter_language_pack'
if ts_pack_dir.exists():
    for f in ts_pack_dir.glob('*.py'):
        datas.append((str(f), 'tree_sitter_language_pack'))
    for f in ts_pack_dir.glob('*.pyi'):
        datas.append((str(f), 'tree_sitter_language_pack'))

# redislite configs (Unix only)
if not is_win:
    redislite_dir = site_packages / 'redislite'
    if redislite_dir.exists():
        for f in redislite_dir.glob('*.conf'):
            datas.append((str(f), 'redislite'))

# ── 3. Hidden imports ────────────────────────────────────────────────────────
hidden_imports = [
    'codegraphcontext',
    'codegraphcontext.cli',
    'codegraphcontext.cli.main',
    'codegraphcontext.cli.cli_helpers',
    'codegraphcontext.cli.config_manager',
    'codegraphcontext.cli.registry_commands',
    'codegraphcontext.cli.setup_wizard',
    'codegraphcontext.cli.setup_macos',
    'codegraphcontext.cli.visualizer',
    'codegraphcontext.core',
    'codegraphcontext.core.database',
    'codegraphcontext.core.database_falkordb',
    'codegraphcontext.core.database_falkordb_remote',
    'codegraphcontext.core.database_kuzu',
    'codegraphcontext.core.falkor_worker',
    'codegraphcontext.core.jobs',
    'codegraphcontext.core.watcher',
    'codegraphcontext.core.cgc_bundle',
    'codegraphcontext.core.bundle_registry',
    'codegraphcontext.server',
    'codegraphcontext.tool_definitions',
    'codegraphcontext.prompts',
    'codegraphcontext.tools',
    'codegraphcontext.tools.code_finder',
    'codegraphcontext.tools.graph_builder',
    'codegraphcontext.tools.package_resolver',
    'codegraphcontext.tools.system',
    'codegraphcontext.tools.scip_indexer',
    'codegraphcontext.tools.scip_pb2',
    'codegraphcontext.tools.advanced_language_query_tool',
    'codegraphcontext.tools.languages',
    'codegraphcontext.tools.languages.python',
    'codegraphcontext.tools.languages.javascript',
    'codegraphcontext.tools.languages.typescript',
    'codegraphcontext.tools.languages.typescriptjsx',
    'codegraphcontext.tools.languages.java',
    'codegraphcontext.tools.languages.go',
    'codegraphcontext.tools.languages.rust',
    'codegraphcontext.tools.languages.c',
    'codegraphcontext.tools.languages.cpp',
    'codegraphcontext.tools.languages.ruby',
    'codegraphcontext.tools.languages.php',
    'codegraphcontext.tools.languages.csharp',
    'codegraphcontext.tools.languages.kotlin',
    'codegraphcontext.tools.languages.scala',
    'codegraphcontext.tools.languages.swift',
    'codegraphcontext.tools.languages.haskell',
    'codegraphcontext.tools.languages.dart',
    'codegraphcontext.tools.languages.perl',
    'codegraphcontext.tools.query_tool_languages.python_toolkit',
    'codegraphcontext.tools.query_tool_languages.javascript_toolkit',
    'codegraphcontext.tools.query_tool_languages.typescript_toolkit',
    'codegraphcontext.tools.query_tool_languages.java_toolkit',
    'codegraphcontext.tools.query_tool_languages.go_toolkit',
    'codegraphcontext.tools.query_tool_languages.rust_toolkit',
    'codegraphcontext.tools.query_tool_languages.c_toolkit',
    'codegraphcontext.tools.query_tool_languages.cpp_toolkit',
    'codegraphcontext.tools.query_tool_languages.ruby_toolkit',
    'codegraphcontext.tools.query_tool_languages.csharp_toolkit',
    'codegraphcontext.tools.query_tool_languages.scala_toolkit',
    'codegraphcontext.tools.query_tool_languages.swift_toolkit',
    'codegraphcontext.tools.query_tool_languages.haskell_toolkit',
    'codegraphcontext.tools.query_tool_languages.dart_toolkit',
    'codegraphcontext.tools.query_tool_languages.perl_toolkit',
    'codegraphcontext.tools.handlers.analysis_handlers',
    'codegraphcontext.tools.handlers.indexing_handlers',
    'codegraphcontext.tools.handlers.management_handlers',
    'codegraphcontext.tools.handlers.query_handlers',
    'codegraphcontext.tools.handlers.watcher_handlers',
    'codegraphcontext.utils.debug_log',
    'codegraphcontext.utils.tree_sitter_manager',
    'codegraphcontext.utils.visualize_graph',

    'kuzu',
    'falkordb',
    'redislite',
    'neo4j',
    'neo4j.io',
    'neo4j.auth_management',
    'neo4j.addressing',
    'neo4j.routing',
    'dotenv',
    'typer',
    'typer.core',
    'typer.main',
    'rich',
    'rich.console',
    'rich.table',
    'rich.progress',
    'rich.markup',
    'rich.panel',
    'tree_sitter',
    'tree_sitter_language_pack',
    'tree_sitter_yaml',
    'tree_sitter_embedded_template',
    'tree_sitter_c_sharp',
    'watchdog',
    'watchdog.observers',
    'watchdog.events',
    'mcp',
    'stdlibs',
    'stdlibs.py3',
    'stdlibs.py312',
    'stdlibs.known',
    'anyio',
    'anyio._backends._asyncio',
    'click',
    'shellingham',
    'httpx',
    'httpcore',
    'importlib.metadata',
    'importlib.util',
    'asyncio',
    'json',
    're',
    'pathlib',
    'threading',
    'subprocess',
    'socket',
    'atexit',
]

# Add platform-specific watchers
if is_win:
    hidden_imports.append('watchdog.observers.read_directory_changes')
elif is_linux:
    hidden_imports.append('watchdog.observers.inotify')
    hidden_imports.append('watchdog.observers.inotify_buffer')
elif is_mac:
    hidden_imports.append('watchdog.observers.fsevents')

# ── 4. Analysis ──────────────────────────────────────────────────────────────
a = Analysis(
    ['cgc_entry.py'],
    pathex=['src'],
    binaries=binaries,
    datas=datas,
    hiddenimports=hidden_imports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        'tkinter', '_tkinter', 'matplotlib', 'numpy', 'pandas', 'scipy',
        'PIL', 'cv2', 'torch', 'tensorflow', 'jupyter', 'notebook', 'IPython',
        'pydoc', 'doctest', 'xmlrpc', 'lib2to3', 'test', 'unittest.mock',
    ],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

# ── 5. ONE-FILE EXE ──────────────────────────────────────────────────────────
exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name='cgc',
    debug=False,
    bootloader_ignore_signals=False,
    strip=not is_win,  # strip fails on windows often
    upx=False,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon=None,
)
