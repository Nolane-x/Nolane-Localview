#include <atk/atk.h>
#include <gtk/gtk-a11y.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define ACCESSIBLE_NAME "LocalView L01 Defunct Button"
#define REPLACEMENT_LABEL "LocalView L02 Replacement Backing"

#define LV_TYPE_RETAINED_BUTTON_ACCESSIBLE (lv_retained_button_accessible_get_type())
#define LV_TYPE_RETAINED_BUTTON (lv_retained_button_get_type())

typedef struct _LvRetainedButtonAccessible {
  GtkButtonAccessible parent_instance;
} LvRetainedButtonAccessible;

typedef struct _LvRetainedButtonAccessibleClass {
  GtkButtonAccessibleClass parent_class;
} LvRetainedButtonAccessibleClass;

typedef struct _LvRetainedButton {
  GtkButton parent_instance;
} LvRetainedButton;

typedef struct _LvRetainedButtonClass {
  GtkButtonClass parent_class;
} LvRetainedButtonClass;

G_DEFINE_TYPE(LvRetainedButtonAccessible,
              lv_retained_button_accessible,
              GTK_TYPE_BUTTON_ACCESSIBLE)
G_DEFINE_TYPE(LvRetainedButton, lv_retained_button, GTK_TYPE_BUTTON)

static guint press_count = 0;
static guint original_press_count = 0;
static guint replacement_press_count = 0;
static GtkWidget *window = NULL;
static GtkWidget *button = NULL;
static GtkWidget *replacement_button = NULL;
static AtkObject *old_accessible = NULL;

static AtkStateSet *
lv_retained_button_accessible_ref_state_set(AtkObject *object) {
  GtkWidget *widget = gtk_accessible_get_widget(GTK_ACCESSIBLE(object));

  if (widget == NULL) {
    /* Mirror GtkAccessible/GtkWidgetAccessible DEFUNCT semantics while
     * deliberately retaining the transport object for this provider seed.
     * The state is derived from the absent backing widget, not from the
     * stdin control channel and not from a fake D-Bus provider. */
    AtkStateSet *state_set = atk_state_set_new();
    atk_state_set_add_state(state_set, ATK_STATE_DEFUNCT);
    return state_set;
  }

  return ATK_OBJECT_CLASS(lv_retained_button_accessible_parent_class)
      ->ref_state_set(object);
}

static void
lv_retained_button_accessible_widget_unset(GtkAccessible *accessible) {
  /* Default GtkAccessible emits state-change::defunct here; atk-adaptor
   * immediately deregisters the object when it observes that event. L01/L02
   * need the equally valid provider lifecycle where the old transport object
   * remains queryable while its backing widget disappears and is later
   * replaced, so this test-only accessible suppresses eager deregistration. */
  (void)accessible;
}

static void
lv_retained_button_accessible_class_init(LvRetainedButtonAccessibleClass *klass) {
  AtkObjectClass *atk_class = ATK_OBJECT_CLASS(klass);
  GtkAccessibleClass *accessible_class = GTK_ACCESSIBLE_CLASS(klass);

  atk_class->ref_state_set = lv_retained_button_accessible_ref_state_set;
  accessible_class->widget_unset = lv_retained_button_accessible_widget_unset;
}

static void
lv_retained_button_accessible_init(LvRetainedButtonAccessible *self) {
  (void)self;
}

static void
lv_retained_button_class_init(LvRetainedButtonClass *klass) {
  gtk_widget_class_set_accessible_type(GTK_WIDGET_CLASS(klass),
                                       LV_TYPE_RETAINED_BUTTON_ACCESSIBLE);
}

static void
lv_retained_button_init(LvRetainedButton *self) {
  (void)self;
}

static void
emit_ready(void) {
  puts("{\"event\":\"ready\"}");
  fflush(stdout);
}

static void
emit_destroyed(void) {
  puts("{\"event\":\"destroyed\"}");
  fflush(stdout);
}

static void
emit_recreated(void) {
  puts("{\"event\":\"recreated\"}");
  fflush(stdout);
}

static void
emit_quitting(void) {
  puts("{\"event\":\"quitting\"}");
  fflush(stdout);
}

static void
emit_status(void) {
  printf("{\"event\":\"status\",\"press_count\":%u,"
         "\"original_press_count\":%u,\"replacement_press_count\":%u}\n",
         press_count,
         original_press_count,
         replacement_press_count);
  fflush(stdout);
}

static void
emit_error(const char *command) {
  fprintf(stdout, "{\"event\":\"error\",\"command\":\"%s\"}\n", command);
  fflush(stdout);
}

static void
on_original_clicked(GtkButton *clicked_button, gpointer user_data) {
  (void)clicked_button;
  (void)user_data;
  press_count += 1;
  original_press_count += 1;
}

static void
on_replacement_clicked(GtkButton *clicked_button, gpointer user_data) {
  (void)clicked_button;
  (void)user_data;
  press_count += 1;
  replacement_press_count += 1;
}

static gboolean
recreate_backing_widget(void) {
  if (old_accessible == NULL || window == NULL || replacement_button != NULL) {
    return FALSE;
  }
  if (gtk_accessible_get_widget(GTK_ACCESSIBLE(old_accessible)) != NULL) {
    return FALSE;
  }

  if (button != NULL && gtk_widget_get_parent(button) == window) {
    gtk_container_remove(GTK_CONTAINER(window), button);
  }
  button = NULL;

  replacement_button = g_object_new(LV_TYPE_RETAINED_BUTTON,
                                    "label", REPLACEMENT_LABEL,
                                    NULL);
  g_signal_connect(replacement_button,
                   "clicked",
                   G_CALLBACK(on_replacement_clicked),
                   NULL);
  gtk_container_add(GTK_CONTAINER(window), replacement_button);

  /* Reattach the retained real GtkButtonAccessible to a new real GTK button.
   * Its AT-SPI transport object remains the same object exported by
   * atk-adaptor; only the backing widget incarnation changes. */
  gtk_accessible_set_widget(GTK_ACCESSIBLE(old_accessible), replacement_button);
  atk_object_set_name(old_accessible, ACCESSIBLE_NAME);
  gtk_widget_show_all(window);
  return TRUE;
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

  if (strcmp(line, "destroy") == 0) {
    if (old_accessible != NULL) {
      gtk_accessible_set_widget(GTK_ACCESSIBLE(old_accessible), NULL);
    }
    emit_destroyed();
  } else if (strcmp(line, "recreate") == 0) {
    if (recreate_backing_widget()) {
      emit_recreated();
    } else {
      emit_error(line);
    }
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
  gtk_window_set_title(GTK_WINDOW(window), "LocalView L01/L02 DEFUNCT Seed");
  gtk_window_set_default_size(GTK_WINDOW(window), 360, 120);
  g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);

  button = g_object_new(LV_TYPE_RETAINED_BUTTON,
                        "label", ACCESSIBLE_NAME,
                        NULL);
  g_signal_connect(button, "clicked", G_CALLBACK(on_original_clicked), NULL);

  old_accessible = gtk_widget_get_accessible(button);
  g_object_ref(old_accessible);
  atk_object_set_name(old_accessible, ACCESSIBLE_NAME);

  gtk_container_add(GTK_CONTAINER(window), button);
  gtk_widget_show_all(window);

  stdin_channel = g_io_channel_unix_new(STDIN_FILENO);
  g_io_add_watch(stdin_channel, G_IO_IN | G_IO_HUP, on_stdin, NULL);

  emit_ready();
  gtk_main();

  g_io_channel_unref(stdin_channel);
  g_clear_object(&old_accessible);
  return 0;
}
