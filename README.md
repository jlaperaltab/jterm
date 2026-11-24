<div align="center">

# jTerm

**Un terminal para macOS: SSH, AWS, archivos y comandos guardados en una sola ventana. Escrito en Rust.**

[![Última versión](https://img.shields.io/github/v/release/jlaperaltab/jterm?label=versi%C3%B3n&color=5c9eff&style=flat-square)](https://github.com/jlaperaltab/jterm/releases/latest)
[![Descargas](https://img.shields.io/github/downloads/jlaperaltab/jterm/total?label=descargas&color=4cc38a&style=flat-square)](https://github.com/jlaperaltab/jterm/releases)
![macOS 13+](https://img.shields.io/badge/macOS-13%2B-lightgrey?style=flat-square&logo=apple)
![Rust](https://img.shields.io/badge/Rust-iced%20%2B%20alacritty__terminal-dea584?style=flat-square&logo=rust&logoColor=white)
![Apple Silicon + Intel](https://img.shields.io/badge/Apple%20Silicon%20%2B%20Intel-universal-8a8f98?style=flat-square)

[**Descargar**](https://github.com/jlaperaltab/jterm/releases/latest) · [Funciones](#-funciones) · [Atajos](#%EF%B8%8F-atajos) · [Instalación](#-instalación) · [Compilar](#-compilar-desde-el-código)

</div>

---

## ✨ Funciones

**Terminal**
- Pestañas con terminales locales y conexiones SSH, con barra lateral de servidores.
- **Paneles divididos** dentro de una pestaña (⌘D a la derecha, ⇧⌘D hacia abajo): una sesión SSH se duplica al mismo servidor; navega con el teclado, redimensiona arrastrando, maximiza un panel y **escribe en todos a la vez**.
- Emulación completa con [`alacritty_terminal`](https://github.com/alacritty/alacritty): colores de 256 y truecolor, ratón para `tmux`, `vim` y `htop`, pantalla alternativa (`nano`, `less`…).
- **Búsqueda en el historial** (⌘F), **enlaces y rutas clicables** (⌘+clic) y **saltos entre prompts** (⌘↑ y ⌘↓).
- Temas de color (jTerm, Dracula, Nord, Tokyo Night, One Dark, Gruvbox, Monokai, Solarized…), fuente, tamaño y cursor configurables.
- Las pestañas de servidores **se colorean según el entorno** (producción, staging…) y la conexión activa se marca en la barra lateral.

**Servidores y AWS**
- Servidores guardados con usuario, puerto, llave y **directorio inicial**; las contraseñas viven en el **Keychain** de macOS, nunca en un archivo.
- **Túneles SSH guardados** (`-L` y `-R`) que se activan con un clic, y **saltos por un bastión** (ProxyJump, hasta 4 servidores encadenados).
- Cuentas AWS con listado de instancias EC2 (con su ID copiable) y conexión mediante EC2 Instance Connect.
- **Explorador de S3**: tu Mac a un lado y los buckets al otro; subir, bajar, crear carpetas, renombrar, eliminar y copiar una **URL firmada**.
- **Visor de logs de CloudWatch**: grupo, rango, filtros por patrón y prefijo de stream, agrupación por invocación, JSON formateado y modo **Seguir** en vivo.

**Archivos**
- Gestor de dos paneles (tu Mac ↔ el servidor) sobre SFTP, con transferencias, renombrar, eliminar y confirmaciones.

**Comandos guardados**
- Una lista de comandos a un atajo de distancia (`⇧⌘K`) para pegarlos o ejecutarlos en la consola activa.
- **Buscador rápido** (`⌘K`) para saltar a un servidor, instancia, pestaña o comando.

**Tus datos, contigo**
- **Sincronización con iCloud** entre tus Macs: conexiones, comandos y ajustes viajan **cifrados** con una contraseña que solo tú conoces.
- Exportar e importar servidores y cuentas, opcionalmente con secretos cifrados.
- Interfaz en **español o inglés** (Ajustes ▸ General).

**Se actualiza solo**
- *jTerm ▸ Buscar actualizaciones…* consulta los releases de este repositorio, verifica el SHA-256 del DMG, instala la nueva versión y se reabre.

> **Aún no incluido** respecto a la versión anterior (en Swift, ya retirada): la grabación de sesión (⌥⌘R) y la confirmación al pegar varias líneas.

## ⌨️ Atajos

| Atajo | Acción |
| --- | --- |
| `⌘T` / `⌘W` | Nueva pestaña / cerrar panel (o la pestaña si tiene un solo panel) |
| `⌥⌘W` | Cerrar la pestaña entera |
| `⌘D` / `⇧⌘D` | Dividir el panel a la derecha / hacia abajo |
| `⌥⌘←→↑↓` | Ir al panel de esa dirección (`⌘]` y `⌘[`: siguiente y anterior) |
| `⇧⌘↩` | Maximizar o restaurar el panel |
| `⌥⌘I` | Escribir en todos los paneles |
| `⇧⌘]` / `⇧⌘[` | Pestaña siguiente / anterior |
| `⌘1` … `⌘9` | Ir a la pestaña N |
| `⌘B` | Mostrar u ocultar la barra lateral |
| `⇧⌘K` | Lista de comandos guardados |
| `⌘K` | Buscador rápido |
| `⌘F` | Buscar en el terminal (`⌘G` siguiente, `⇧⌘G` anterior) |
| `⌘↑` / `⌘↓` | Prompt anterior / siguiente |
| `⌘,` | Ajustes |

## 📦 Instalación

1. Descarga el último `jTerm-X.Y.Z.dmg` desde [Releases](https://github.com/jlaperaltab/jterm/releases/latest).
2. Ábrelo y arrastra **jTerm** a **Aplicaciones**.
3. La primera vez, macOS dirá que no puede verificar la app, porque no está notarizada por Apple. Es normal. Para abrirla:
   - **macOS 15 o posterior:** *Ajustes del Sistema ▸ Privacidad y seguridad* ▸ *Abrir de todos modos*.
   - **macOS 13 o 14:** clic derecho sobre jTerm ▸ *Abrir* ▸ *Abrir*.
   - **O desde Terminal:**
     ```sh
     xattr -dr com.apple.quarantine /Applications/jTerm.app
     ```

Solo hace falta la primera vez: las actualizaciones posteriores se instalan desde la propia app.

**Requisitos:** macOS 13 o superior, Apple Silicon o Intel.

**Dónde se guardan tus datos:** en `~/Library/Application Support/jTerm`. Las contraseñas y claves secretas van al Keychain de macOS.

## 🛠 Compilar desde el código

Necesitas [Rust](https://rustup.rs) (estable) y las Command Line Tools de Xcode (`xcode-select --install`).

```sh
git clone https://github.com/jlaperaltab/jterm.git
cd jterm/rust

cargo run -p jterm-app --release       # ejecutar
cargo test --workspace                 # pruebas (no tocan AWS ni servidores reales)

VERSION=1.0.0 ./build_app.sh           # empaqueta build/jTerm.app
VERSION=1.0.0 ./make_dmg.sh            # DMG universal en dist/
./release.sh 1.0.0                     # publica el release en GitHub (necesita `gh`)
```

| Variable | Efecto |
| --- | --- |
| `VERSION` | Número de versión de la app (también se usa para comparar actualizaciones). |
| `UNIVERSAL=1` | Binario para Apple Silicon e Intel. |
| `GITHUB_REPO` | `usuario/repo` donde el actualizador busca releases (por defecto `jlaperaltab/jterm`). |
| `JTERM_DATA_DIR` | Carpeta de datos alternativa, útil para pruebas. |

### Estructura

El proyecto es un workspace de Cargo en `rust/`:

| Crate | Contenido |
| --- | --- |
| `jterm-core` | Modelos, ajustes, temas, cifrado, firma SigV4, AWS/S3/CloudWatch, sincronización, SFTP y túneles. Sin interfaz. |
| `jterm-net` | Cliente HTTP real (`reqwest`) y actualizador, tras un trait `Transport` que permite probar sin red. |
| `jterm-app` | La aplicación: interfaz con [`iced`](https://iced.rs), terminal, menú nativo y paneles. |

## 🙏 Créditos

- [iced](https://iced.rs) y [alacritty_terminal](https://github.com/alacritty/alacritty), bajo licencias MIT y Apache-2.0.
- [Lucide](https://lucide.dev) para los iconos.
