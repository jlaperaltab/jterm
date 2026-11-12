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
