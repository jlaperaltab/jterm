# Parche de jTerm sobre SwiftTerm 1.11.2

Copia local de https://github.com/migueldeicaza/SwiftTerm (v1.11.2, MIT) con un cambio en `Sources/SwiftTerm/Buffer.swift`:

- `Buffer.init`: `marginLeft/marginRight` se inicializan al ancho completo (antes `marginRight` quedaba en 0).
- `Buffer.resize`: sin modo de márgenes laterales (DECLRMM) el margen derecho sigue el ancho de la pantalla.

Síntoma corregido: en la pantalla alternativa (nano, vim, less…), `CSI n T`/`S`, `CSI n L`/`M` con región de
desplazamiento copiaban y limpiaban una sola columna, dejando restos de texto al pegar o insertar líneas.

- `Mac/MacTerminalView.swift` (`setFrameSize`): el cambio de tamaño de la cuadrícula se limita a una vez cada
  `resizeThrottle` (0,12 s) mientras la ventana se redimensiona; el primero se aplica al instante y el tamaño final
  siempre se aplica. Evita que programas de pantalla completa (nano, vim…) redibujen decenas de veces por segundo
  y parpadeen al arrastrar el borde de la ventana.

- `Mac/MacTerminalView.swift` (`keyDown`): con "Option como Meta" activo, Option+tecla se envía al sistema (no como ESC+tecla) cuando
  la distribución produce un carácter ASCII (AltGr en teclado español/latino: Option+2 → @, Option+1 → |, Option+3 → #, Option+` → [ …)
  o una tecla muerta de símbolo (Option+ñ → ~). Option+letra en US sigue siendo Meta (ESC+letra), y Option+←/→ salta palabras.

- `Mac/MacTerminalView.swift` (`returnInterceptor`, `doCommand`): gancho opcional que se consulta al pulsar Retorno; si devuelve `false`
  el Retorno no se envía. jTerm lo usa para avisar antes de ejecutar comandos delicados (`rm`, `drop`…) en pestañas de producción.

- `Mac/MacFindBarView.swift`: textos de la barra de búsqueda (⌘F) traducidos al español.
- `Mac/MacTerminalView.swift` (`linkClickInterceptor`, `mouseUp`): gancho para ⌘+clic sobre texto que no es un enlace OSC 8
  (jTerm lo usa para abrir URLs y rutas que aparecen en la salida).
- `Mac/JTermExtras.swift`: filas absolutas y desplazamiento (`jtermCursorAbsoluteRow`, `jtermScroll(toAbsoluteRow:)`…)
  para saltar entre prompts.

- `Apple/AppleTerminalView.swift` (`scrollTo(row:)`): marca `terminal.userScrolling` cuando la vista queda por encima del final.
  Sin esto, mientras un comando seguía imprimiendo no se podía subir con la rueda: cada línea nueva devolvía la vista al final.

- `Mac/MacTerminalView.swift` (`becomeFirstResponder`, `keyDown`): pasan de `public` a `open` para poder sobrescribirlos desde jTerm
  (marcar el panel activo al recibir el foco y reenviar lo escrito a los demás paneles).

- `Mac/MacTerminalView.swift` (`jtermResizeLocked`, `applySizeNow`): bloqueo de cambios de tamaño mientras jTerm reorganiza los paneles
  (la vista pasaba por tamaños como 1×2 que llegaban al servidor y descuadraban el prompt de bash), y las vistas de menos de 40×30 puntos
  no redimensionan la pantalla.
