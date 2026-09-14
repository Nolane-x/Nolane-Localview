#include <atk/atk.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define TARGET_NAME "LocalView L03 Visible Target"
#define BLOCKER_NAME "LocalView L03 Occluding Blocker"

static GtkWidget *window = NULL;
static GtkWidget *overlay = NULL;
static GtkWidget *target = NULL;
static GtkWidget *blocker = NULL;
static guint target_press_count = 0;

static void
emit_ready(void) {
  puts("{\"event\":\"ready\",\"blocker_present\":true}");
  fflush(stdout);
}

static void
emit_unblocked(void) {
  puts("{\"event\":\"unblocked\",\"blocker_present\":false}");
  fflush(stdout);
}

static void
emit_quitting(void) {
  puts("{\"event\":\"quitting\"}");
  fflush(stdout);
}

static void
emit_status(void) {
  printf("{\"event\":\"status\",\"target_press_count\":%u,"
         "\"blocker_present\":%s,\"target_name\":\"%s\","
         "\"blocker_name\":\"%s\"}\n",
         target_press_count,
         blocker != NULL ? "true" : "false",
         TARGET_NAME,
         BLOCKER_NAME);
  fflush(stdout);
}

static void
emit_error(const char *command) {
  fprintf(stdout, "{\"event\":\"error\",\"command\":\"%s\"}\n", command);
  fflush(stdout);
}

static void
on_target_clicked(GtkButton *button, gpointer user_data) {
  (void)button;
  (void)user_data;
  target_press_count += 1;
}

static void
remove_blocker(void) {
  if (blocker == NULL) {
    return;
  }

  gtk_container_remove(GTK_CONTAINER(overlay), blocker);
  blocker = NULL;
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

  if (strcmp(line, "status") == 0) {
    emit_status();
  } else if (strcmp(line, "unblock") == 0) {
    remove_blocker();
    emit_unblocked();
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
  AtkObject *target_accessible;
  AtkObject *blocker_accessible;

  gtk_init(&argc, &argv);

  window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
  gtk_window_set_title(GTK_WINDOW(window), "LocalView L03 Visible Occlusion Seed");
  gtk_window_set_default_size(GTK_WINDOW(window), 420, 220);
  g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);

  overlay = gtk_overlay_new();
  gtk_container_add(GTK_CONTAINER(window), overlay);

  target = gtk_button_new_with_label(TARGET_NAME);
  gtk_widget_set_hexpand(target, TRUE);
  gtk_widget_set_vexpand(target, TRUE);
  gtk_widget_set_size_request(target, 320, 140);
  g_signal_connect(target, "clicked", G_CALLBACK(on_target_clicked), NULL);

  target_accessible = gtk_widget_get_accessible(target);
  atk_object_set_name(target_accessible, TARGET_NAME);
  gtk_container_add(GTK_CONTAINER(overlay), target);

  /* A real GTK overlay child covers the target's center. The target remains
   * present, VISIBLE and SHOWING in AT-SPI, while the parent Component
   * hit-test resolves this blocker. Production LocalView observes only the
   * real AT-SPI state/component interfaces; stdin is lifecycle control for
   * the validation seed and is never a production truth source. */
  blocker = gtk_button_new_with_label(BLOCKER_NAME);
  gtk_widget_set_halign(blocker, GTK_ALIGN_FILL);
  gtk_widget_set_valign(blocker, GTK_ALIGN_FILL);
  gtk_widget_set_hexpand(blocker, TRUE);
  gtk_widget_set_vexpand(blocker, TRUE);
  gtk_widget_set_size_request(blocker, 320, 140);
  blocker_accessible = gtk_widget_get_accessible(blocker);
  atk_object_set_name(blocker_accessible, BLOCKER_NAME);
  gtk_overlay_add_overlay(GTK_OVERLAY(overlay), blocker);
  gtk_overlay_set_overlay_pass_through(GTK_OVERLAY(overlay), blocker, FALSE);

  gtk_widget_show_all(window);

  stdin_channel = g_io_channel_unix_new(STDIN_FILENO);
  g_io_add_watch(stdin_channel, G_IO_IN | G_IO_HUP, on_stdin, NULL);

  emit_ready();
  gtk_main();

  g_io_channel_unref(stdin_channel);
  return 0;
}
