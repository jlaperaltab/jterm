<div align="center">

# jTerm

**Un terminal para macOS hecho a mano: SSH, AWS, archivos y comandos guardados en una sola ventana.**

[![Última versión](https://img.shields.io/github/v/release/jlaperaltab/jterm?label=versi%C3%B3n&color=5c9eff&style=flat-square)](https://github.com/jlaperaltab/jterm/releases/latest)
[![Descargas](https://img.shields.io/github/downloads/jlaperaltab/jterm/total?label=descargas&color=4cc38a&style=flat-square)](https://github.com/jlaperaltab/jterm/releases)
![macOS 13+](https://img.shields.io/badge/macOS-13%2B-lightgrey?style=flat-square&logo=apple)
![Swift 5.9](https://img.shields.io/badge/Swift-5.9-F05138?style=flat-square&logo=swift&logoColor=white)
![Apple Silicon + Intel](https://img.shields.io/badge/Apple%20Silicon%20%2B%20Intel-universal-8a8f98?style=flat-square)

[**Descargar**](https://github.com/jlaperaltab/jterm/releases/latest) · [Funciones](#-funciones) · [Instalación](#-instalación) · [Compilar](#-compilar-desde-el-código)

</div>

---

## ✨ Funciones

**Terminal**
- Pestañas con terminales locales y conexiones SSH, con barra lateral de servidores.
- **Paneles divididos** dentro de una pestaña (⌘D a la derecha, ⇧⌘D hacia abajo): una sesión SSH se duplica al mismo servidor, y se puede navegar con el teclado, redimensionar arrastrando, maximizar un panel y **escribir en todos los paneles a la vez**.
- Emulación completa basada en [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm): colores de 256 y truecolor, ratón para `tmux`, `vim` y `htop`, pantalla alternativa (`nano`, `less`…).
- **Búsqueda en el historial** (⌘F), **enlaces y rutas clicables** (⌘+clic abre la URL o el archivo; en una conexión SSH, la ruta se muestra en el gestor de archivos del servidor) y **saltos entre prompts** (⌘↑ y ⌘↓).
- **Grabar la sesión a un archivo de texto** limpio (⌥⌘R), o automáticamente en las pestañas de producción.
- Temas de color (jTerm, Dracula, Nord, Tokyo Night, One Dark, Gruvbox, Monokai, Solarized…), fuentes monoespaciadas, tamaño, cursor y líneas de historial configurables.
- **Option como Meta** para saltar palabras, y **AltGr** que sigue escribiendo `@`, `|`, `#` o `~` en teclados español y latino.

**Servidores y AWS**
- Servidores guardados con usuario, puerto y llave; las contraseñas viven en el **Keychain** de macOS, nunca en un archivo.
- **Túneles SSH guardados** (`-L` y `-R`) que se activan con un clic, y **saltos por un bastión** (ProxyJump, hasta 4 servidores encadenados) con la llave y la contraseña de cada servidor.
- Cuentas AWS con listado de instancias EC2 y conexión mediante EC2 Instance Connect.
- **Explorador de S3**: tu Mac a un lado y los buckets de la cuenta al otro. Navega, sube y baja archivos y carpetas (arrastrando o con las flechas), crea carpetas, renombra y elimina, y copia una **URL firmada** para compartir un objeto.
- **Visor de logs de CloudWatch**: elige grupo y rango, filtra con patrones, filtra por prefijo de stream y activa **Seguir** para ver los eventos nuevos en vivo.

**Archivos**
- Gestor de archivos de dos paneles (tu Mac ↔ el servidor) sobre SFTP, con transferencias, renombrar, eliminar y confirmaciones.

**Comandos guardados**
- Una lista de comandos a un atajo de distancia (`⇧⌘K`) para pegarlos o ejecutarlos en la consola activa.

**Tus datos, contigo**
- **Sincronización con iCloud** entre tus Macs: conexiones, comandos y ajustes viajan **cifrados** con una contraseña que solo tú conoces.
- Exportar e importar servidores y cuentas, opcionalmente con secretos cifrados.

**Se actualiza solo**
- *jTerm ▸ Buscar actualizaciones…* consulta los releases de este repositorio, verifica el SHA-256 del DMG, instala la nueva versión y se reabre.
- Revisa en silencio al abrir, como máximo una vez al día.

## ⌨️ Atajos

| Atajo | Acción |
| --- | --- |
| `⌘T` / `⌘W` | Nueva pestaña / cerrar panel (o la pestaña si tiene un solo panel) |
| `⌘D` / `⇧⌘D` | Dividir el panel a la derecha / hacia abajo |
| `⌥⌘←→↑↓` | Ir al panel de esa dirección (`⌘]` y `⌘[`: siguiente y anterior) |
| `⇧⌘↩` | Maximizar o restaurar el panel |
| `⌥⌘I` | Escribir en todos los paneles |
| `⇧⌘]` / `⇧⌘[` | Pestaña siguiente / anterior |
| `⌘1` … `⌘9` | Ir a la pestaña N |
| `⌘B` | Mostrar u ocultar la barra lateral |
| `⇧⌘K` | Lista de comandos guardados |
| `⌘K` | Buscar servidor, instancia, pestaña o comando |
| `⌘F` | Buscar en el terminal (`⌘G` siguiente, `⇧⌘G` anterior) |
| `⌘↑` / `⌘↓` | Prompt anterior / siguiente |
| `⌥⌘R` | Grabar o detener la grabación de la sesión |
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

Solo necesitas las Command Line Tools de Xcode (`xcode-select --install`); no hace falta Xcode.

```sh
git clone https://github.com/jlaperaltab/jterm.git
cd jterm

swift run                              # ejecutar en modo desarrollo
VERSION=0.2.0 ./build_app.sh           # empaqueta build/jTerm.app
```

| Variable | Efecto |
| --- | --- |
| `VERSION` | Número de versión de la app (también se usa para comparar actualizaciones). |
| `UNIVERSAL=1` | Binario para Apple Silicon e Intel. |
| `ADHOC=1` | Firma ad-hoc para compartir con otros Macs. |
| `GITHUB_REPO` | `usuario/repo` donde el actualizador busca releases (por defecto `jotaele/jterm`). |

> Si ejecutas con `swift run`, la app no tiene `Info.plist` y *Buscar actualizaciones* avisará que solo funciona desde la app empaquetada.

Para que macOS no vuelva a pedir la clave del Keychain en cada recompilación local, crea una vez un certificado de firma propio:

```sh
tools/make_signing_cert.sh
```

## 🙏 Créditos

- [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) de Miguel de Icaza, bajo licencia MIT, incluido en `Vendor/` con los cambios descritos en [`JTERM_PATCH.md`](Vendor/SwiftTerm/JTERM_PATCH.md).
