# tonycode

App nativa y ligera (Rust + egui, sin navegador embebido) para organizar proyectos y correr sesiones de Claude Code con varias cuentas.

## Instalar (macOS)

```bash
unzip tonycode.zip && cd tonycode
./install.sh
```

El script instala Rust si no lo tienes, compila en modo release y crea `~/Applications/tonycode.app`.
Para actualizar después de cambios: vuelve a correr `./install.sh`.

## Qué hace

- **Proyectos en grupos** (Trabajo, Freelance, Personal...). `+ proyecto` abre el selector de carpeta. Flechas para reordenar; clic derecho para cambiar la cuenta, mover a otro grupo o quitar.
- **Varios chats por proyecto.** Cada chat es una sesión real de `claude` en una terminal integrada. Clic derecho en un chat: renombrar, reiniciar, detener, eliminar.
- **Cuenta Claude por proyecto** (`work` / `personal`): se lanza con `CLAUDE_CONFIG_DIR=~/.claude-work` o `~/.claude-personal`. Arriba a la derecha eliges la cuenta por defecto para proyectos nuevos.
- **Sesiones retomables:** cada chat tiene su ID de sesión. Al reabrir la app usa `claude --resume <id>`.
- **Worktree por chat** (casilla `worktree` junto a `+ Chat`): crea la rama `tonycode/chat-N-xxxx` en `~/.tonycode/worktrees/<proyecto>/`.
- **Sub agentes:** se leen del archivo de sesión de Claude (llamadas a Task/Agent) y se muestran bajo el chat y en la franja superior.
- **Te necesita:** detecta cuando Claude pide permiso o termina en un chat que no estás viendo. Lo marca en la barra lateral y manda una notificación de macOS si la app no está al frente.
- **Archivos:** panel con navegador y editor simple (Cmd+S guarda).
- **Cambios:** `git diff` del chat, con Commit y Revertir (pide confirmación).
- **GitHub:** abajo a la derecha. Lee los hosts de `~/.ssh/config` con `HostName github.com`. Al elegir uno, aplica `git config core.sshCommand "ssh -i <llave> -o IdentitiesOnly=yes"` en el repo del proyecto y lo recuerda por proyecto.

## Bots líderes

Un bot es una sesión de Claude que coordina tus proyectos. Le das tickets ("tenemos DNG-2395, DNG-2400 y DNG-2410") y él decide a qué repo va cada uno y los delega.

- **Crear:** `+ bot` en la barra lateral. Eliges nombre, cuenta de Claude (work o personal), a qué proyectos puede delegar, si cada tarea va en su propio worktree y si puede responder prompts de permiso (apagado por defecto).
- **Cómo delega:** el bot trabaja en `~/.tonycode/bots/<id>/`, con un `CLAUDE.md` que lista tus proyectos y un comando `./tc`:
  - `./tc projects` lista los proyectos que puede usar.
  - `./tc delegate <proyecto> --title "DNG-2395: ..." --prompt "..."` crea un chat en ese proyecto, con su rama, y lo arranca con ese prompt.
  - `./tc status` muestra el estado de cada tarea (running, idle, needs, exited) y sus últimas líneas.
  - `./tc tail <id>` y `./tc send <id> "mensaje"` sirven para dar seguimiento.
- **Cuentas:** el bot usa su propia cuenta. Cada tarea delegada usa la cuenta del proyecto donde corre.
- **Permisos:** si una tarea pide permiso, aparece en Te necesita y el bot no puede aprobarla, salvo que actives "Puede responder prompts de permiso" en ese bot.
- Las tareas aparecen bajo el bot y dentro de su proyecto, marcadas "delegado por <bot>".

## Actualizaciones

La app guarda el commit con el que se compiló y la carpeta del código. Al abrir y cada 30 minutos hace `git fetch` ahí. Si `master` en el remoto tiene commits nuevos, aparece **Actualización disponible (n)** en la barra de estado: un clic muestra los commits y **Actualizar ahora** hace `git pull`, compila, reinstala y te deja reiniciar.

Para buscar al instante, haz clic en la versión (`v0.1.0 · abc1234`) en la barra de estado.

Flujo: haz cambios donde quieras, `git push` a master, y la app se actualiza sola desde su carpeta de código.

## Atajos

| Atajo | Acción |
|---|---|
| Cmd+K | Buscar proyecto o chat |
| Cmd+T | Nuevo chat en el proyecto actual |
| Cmd+S | Guardar archivo en el editor |
| Cmd+V | Pegar en la terminal |
| Esc | Cerrar panel de GitHub |
| Seleccionar con el mouse | Copia al portapapeles automáticamente |
| Doble clic / triple clic | Copia la palabra / la línea |
| Rueda o trackpad | Scroll del historial (o se envía a Claude si usa el mouse) |
| Shift+PgUp / Shift+PgDn | Scroll del historial por páginas |

El teclado va a la terminal siempre que no estés escribiendo en un campo de texto.

## Configuración

Todo vive en `~/.tonycode/tonycode.toml` (se guarda solo). Opciones útiles:

```toml
claude_cmd = "claude"     # ruta o nombre del binario
login_shell = true        # lanza claude vía tu shell (-lic) para heredar PATH de nvm/bun
notify = true             # notificaciones de macOS

[[accounts]]              # agrega más cuentas Claude si quieres
name = "work"
config_dir = "~/.claude-work"

[[gh_identities]]         # opcional: user.name / user.email por cuenta de GitHub
host = "github.com-aceable"
user = "oswaldosanchezace"
email = "tu@correo.com"
```

## Limitaciones conocidas (v0.1)

- Reordenar proyectos es con flechas, no con arrastrar.
- Al eliminar un chat con worktree, la carpeta del worktree se conserva (bórrala con `git worktree remove <ruta>`).
