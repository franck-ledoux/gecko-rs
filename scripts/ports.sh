# Port checks shared by ./dev and ./view.
#
# Both scripts need a fixed port — 5173 so a bookmark keeps working, and the
# station's so the dev server can be told in advance where to forward. A fixed
# port means a leftover process from an earlier run is in the way, and the two
# ways that goes wrong are both quiet:
#
#   * Vite exits on "port already in use", but it was started in the
#     background, so the script sails on and the readiness check is answered
#     by the *leftover* server.
#   * That leftover answers the page just as happily and forwards nowhere, so
#     the view comes up looking perfectly healthy and ignores every command
#     typed in the console.
#
# Hence: look before starting, and say what is in the way.

# Who is listening on a port, as "pid<TAB>command line", or nothing if it is
# free. The command line comes from ps rather than from lsof, which reports
# only the executable's name — and "node" names nothing on a machine that is
# running several.
port_holder() {
  local pid
  if command -v lsof >/dev/null; then
    pid=$(lsof -nP -iTCP:"$1" -sTCP:LISTEN -t 2>/dev/null | head -1)
    [ -n "$pid" ] || return 0
    printf '%s\t%s\n' "$pid" "$(ps -o command= -p "$pid" 2>/dev/null || echo unknown)"
  else
    # No lsof: settle for knowing that *something* is there.
    (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null && printf '?\tunknown\n'
  fi
}

# Refuse to start on an occupied port, and leave the reader one command to run.
require_free() {
  local port=$1 what=$2 holder pid command
  holder=$(port_holder "$port") || true
  [ -n "$holder" ] || return 0

  pid=${holder%%	*}
  command=${holder#*	}
  {
    echo
    echo "Port $port is taken, and $what needs it."
    echo
    echo "    pid $pid — $command"
    echo
    if [ "$pid" = "?" ]; then
      echo "  Stop whatever is on that port and try again."
    else
      echo "  Most likely a dev server left behind by an earlier ./dev or"
      echo "  ./view whose terminal went away. To stop it:"
      echo
      echo "      kill $pid"
    fi
    echo
  } >&2
  exit 1
}
