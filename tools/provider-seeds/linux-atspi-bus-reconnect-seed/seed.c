#include <atk/atk.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define ACCESSIBLE_NAME "LocalView L05 Bus Reconnect Button"

static guint press_count = 0;

static void emit_ready(void) {
  puts("{\"event\":\"ready\"}");
  fflush(stdout);
}

static void emit_status(void) {
  printf("{\"event\":\"status\",\"press_count\":%u}\n", press_count);
  fflush(stdout);
}

static void emit_quitting(void) {
  puts("{\"event\":\"quitting\"}");
  fflush(stdout);
}

static void emit_error(const char *command) {
  fprintf(stdout, "{\"event\":\"error\",\"command\":\"%s\"}\n", command);
  fflush(stdout);
}

static void on_clicked(GtkButton *button, gpointer user_data) {
  (void)button;
  (void)user_data;
  press_count += 1;
}

static gboolean on_stdin(GIOChannel *source, GIOCondition condition, gpointer user_data) {
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

int main(int argc, char **argv) {
  GIOChannel *stdin_channel;
  GtkWidget *window;
  GtkWidget *button;
  AtkObject *accessible;

  gtk_init(&argc, &argv);

  window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
  gtk_window_set_title(GTK_WINDOW(window), "LocalView L05 Bus Reconnect Seed");
  gtk_window_set_default_size(GTK_WINDOW(window), 360, 120);
  g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);

  button = gtk_button_new_with_label(ACCESSIBLE_NAME);
  g_signal_connect(button, "clicked", G_CALLBACK(on_clicked), NULL);
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
