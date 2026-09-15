#include <atk/atk.h>
#include <gtk/gtk-a11y.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define ACCESSIBLE_NAME "LocalView L04 Missed Event Button"

#define LV_TYPE_L04_BUTTON_ACCESSIBLE (lv_l04_button_accessible_get_type())
#define LV_TYPE_L04_BUTTON (lv_l04_button_get_type())

typedef struct _LvL04ButtonAccessible {
  GtkButtonAccessible parent_instance;
} LvL04ButtonAccessible;

typedef struct _LvL04ButtonAccessibleClass {
  GtkButtonAccessibleClass parent_class;
} LvL04ButtonAccessibleClass;

typedef struct _LvL04Button {
  GtkButton parent_instance;
} LvL04Button;

typedef struct _LvL04ButtonClass {
  GtkButtonClass parent_class;
} LvL04ButtonClass;

G_DEFINE_TYPE(LvL04ButtonAccessible,
              lv_l04_button_accessible,
              GTK_TYPE_BUTTON_ACCESSIBLE)
G_DEFINE_TYPE(LvL04Button, lv_l04_button, GTK_TYPE_BUTTON)

static gboolean checked_state = FALSE;
static GtkWidget *window = NULL;
static GtkWidget *button = NULL;
static AtkObject *accessible = NULL;

static AtkStateSet *
lv_l04_button_accessible_ref_state_set(AtkObject *object) {
  AtkStateSet *state_set =
      ATK_OBJECT_CLASS(lv_l04_button_accessible_parent_class)->ref_state_set(object);

  /* L04 deliberately models a toolkit bug where the provider's direct state
   * query is correct but the corresponding state-change notification is
   * missing. The backing state is therefore visible through the real GTK ->
   * ATK -> atk-adaptor -> AT-SPI GetState path, while mutate_silent does not
   * emit atk_object_notify_state_change(). */
  if (checked_state) {
    atk_state_set_add_state(state_set, ATK_STATE_CHECKED);
  } else {
    atk_state_set_remove_state(state_set, ATK_STATE_CHECKED);
  }
  return state_set;
}

static void
lv_l04_button_accessible_class_init(LvL04ButtonAccessibleClass *klass) {
  ATK_OBJECT_CLASS(klass)->ref_state_set =
      lv_l04_button_accessible_ref_state_set;
}

static void
lv_l04_button_accessible_init(LvL04ButtonAccessible *self) {
  (void)self;
}

static void
lv_l04_button_class_init(LvL04ButtonClass *klass) {
  gtk_widget_class_set_accessible_type(GTK_WIDGET_CLASS(klass),
                                       LV_TYPE_L04_BUTTON_ACCESSIBLE);
}

static void
lv_l04_button_init(LvL04Button *self) {
  (void)self;
}

static void
emit_ready(void) {
  puts("{\"event\":\"ready\"}");
  fflush(stdout);
}

static void
emit_control_emitted(void) {
  puts("{\"event\":\"control_emitted\"}");
  fflush(stdout);
}

static void
emit_mutated(void) {
  puts("{\"event\":\"mutated_silent\"}");
  fflush(stdout);
}

static void
emit_status(void) {
  printf("{\"event\":\"status\",\"checked\":%s}\n",
         checked_state ? "true" : "false");
  fflush(stdout);
}

static void
emit_quitting(void) {
  puts("{\"event\":\"quitting\"}");
  fflush(stdout);
}

static void
emit_error(const char *command) {
  fprintf(stdout, "{\"event\":\"error\",\"command\":\"%s\"}\n", command);
  fflush(stdout);
}

static gboolean
on_stdin(GIOChannel *source, GIOCondition condition, gpointer user_data) {
  gchar *line = NULL;
  gsize length = 0;
  GError *error = NULL;
  GIOStatus status;

  (void)user_data;

  if ((condition & G_IO_HUP) != 0) {
    gtk_main_quit();
    return G_SOURCE_REMOVE;
  }

  status = g_io_channel_read_line(source, &line, &length, NULL, &error);
  if (status == G_IO_STATUS_EOF) {
    gtk_main_quit();
    return G_SOURCE_REMOVE;
  }
  if (status == G_IO_STATUS_ERROR) {
    if (error != NULL) {
      fprintf(stderr, "stdin read failed: %s\n", error->message);
      g_error_free(error);
    }
    gtk_main_quit();
    return G_SOURCE_REMOVE;
  }
  if (status != G_IO_STATUS_NORMAL || line == NULL) {
    g_free(line);
    return G_SOURCE_CONTINUE;
  }

  g_strchomp(line);

  if (strcmp(line, "emit_control") == 0) {
    /* Positive control: prove the independent AT-SPI listener is actually
     * subscribed to state-change signals from this exact real accessible. */
    atk_object_notify_state_change(accessible, ATK_STATE_BUSY, TRUE);
    emit_control_emitted();
  } else if (strcmp(line, "mutate_silent") == 0) {
    /* The state changes without emitting the corresponding toolkit event. */
    checked_state = TRUE;
    emit_mutated();
  } else if (strcmp(line, "status") == 0) {
    emit_status();
  } else if (strcmp(line, "quit") == 0) {
    emit_quitting();
    g_free(line);
    gtk_main_quit();
    return G_SOURCE_REMOVE;
  } else {
    emit_error(line);
  }

  g_free(line);
  return G_SOURCE_CONTINUE;
}

int
main(int argc, char **argv) {
  GIOChannel *stdin_channel;

  gtk_init(&argc, &argv);

  window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
  gtk_window_set_title(GTK_WINDOW(window), "LocalView L04 Missed Event Seed");
  gtk_window_set_default_size(GTK_WINDOW(window), 380, 120);
  g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);

  button = g_object_new(LV_TYPE_L04_BUTTON,
                        "label", ACCESSIBLE_NAME,
                        NULL);
  accessible = gtk_widget_get_accessible(button);
  atk_object_set_name(accessible, ACCESSIBLE_NAME);

  gtk_container_add(GTK_CONTAINER(window), button);
  gtk_widget_show_all(window);

  stdin_channel = g_io_channel_unix_new(STDIN_FILENO);
  g_io_add_watch(stdin_channel, G_IO_IN | G_IO_HUP, on_stdin, NULL);

  emit_ready();
  gtk_main();

  g_io_channel_unref(stdin_channel);
  return 0;
}
