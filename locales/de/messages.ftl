# comment no Tabs, please

describe-yourself = Du bist ein hilfreicher Assistent.
  Benutzernachricht: {$p1}
  Sage deinen Namen und antworte dann!

which-task-for-you = Für welche dieser Aufgaben bist du geeignet?

three-qwestions = Ich brauche deine Hilfe bei drei Arten von Aufgaben!
  1. Verstehen, was auf dem Bild zu sehen ist.
  2. Arbeiten mit Werkzeugen.
  3. Nachdenken.

object-words = bauen konstruieren objekt erstellen machen
document-words = Bild Bildaufnahme Video Bericht Dokument Datei
description-words = Modifikation Veränderung beschreiben
comparison-words = vergleichen unterscheiden erkennen aktualisieren ändern
last-words = letzte vorherige jüngste
new-words = neu neueste
all-words = alle jede gesamte vollständige
period-words = tag woche monat quartal jahr
amount_num = 1 2 3 4 5 6 7 8 9 10
amount_text = eins zwei drei vier fünf sechs sieben acht neun zehn

# -------------------------------

# Progress Messages
progress-analyzing = Analysiere Ihre Anfrage...
progress-context-validation = Validiere Kontext...
progress-formatting = Formatiere Antwort...

# Context Request Messages
context-request-object-id = Welches Gebäude oder welche Baustelle möchten Sie bearbeiten?
context-request-current-report = Welchen Fotobericht möchten Sie analysieren?
context-request-previous-report = Mit welchem vorherigen Bericht möchten Sie vergleichen?
context-request-clarification = Ich bin mir nicht sicher, was Sie meinen. Könnten Sie das bitte klarstellen?

# UI Messages
status-not-set = NICHT GESETZT
status-set = {$value}
no-conversation-history = Keine vorherige Konversation
no-worker-results = Noch keine Worker ausgeführt
worker-result-summary = {$worker_type}: {$status} ({$execution_time}ms)

# Error Messages
error-serialization = Serialisierungsfehler: {$error}
error-agent = Agentenfehler: {$error}
error-classification = Klassifizierungsergebnis konnte nicht geparst werden: {$error}
error-classification-fallback = Klassifizierungsergebnis konnte nicht geparst werden: {$error}
error-unknown-decision = Unbekannter Entscheidungstyp: {$decision_type}
error-missing-field = Fehlendes Feld {$field}
error-unknown-worker = Unbekannter Worker-Typ
error-unknown-context-field = Unbekanntes Kontextfeld
error-unknown-decision-type = Unbekannter Entscheidungstyp: {$decision_type}
error-empty-report-id = Report-ID darf nicht leer sein.

# Orchestrator Messages
orchestrator-cannot-process = Diese Anfrage kann nicht verarbeitet werden.

# Response Formatter Messages
error-comparison-parse = Vergleich konnte nicht geparst werden: {$error}

# General Messages
analyzing-query = Analysiere Anfrage.
fetching-data = Lade Daten.
processing-results = Verarbeite Ergebnisse.

context-not-set = nicht_gesetzt
progress-executing-worker = Führe { $worker } aus...
progress-worker-finding-object    = Objekt im Projektbaum wird gesucht...
progress-worker-finding-reports   = Passende Reports werden gesucht...
progress-worker-loading-tree      = Objekthierarchie wird geladen...
progress-worker-loading-reports   = Reportliste wird geladen...
progress-worker-describing-report = Report wird analysiert...
progress-worker-comparing-reports = Beide Reports werden verglichen...
progress-worker-searching-knowledge = Projektwissen wird durchsucht...
# ------------------------------------------------------------
progress-downloading-image = Bild '{ $report_type }' wird heruntergeladen...
progress-processing-image = Bild '{ $report_type }' wird verarbeitet...
progress-generating-description = Beschreibung für '{ $report_type }' wird erstellt...
progress-description-parse-warning = Beschreibung für '{ $report_type }' konnte nicht verarbeitet werden, Rohdaten gespeichert
progress-generate-err = Beschreibung für '{ $report_type }' konnte nicht generiert werden
processing = Wird verarbeitet...
unknown = Unbekannt
# ------------------------------------------------------------
# Info / status messages
# ------------------------------------------------------------

info-no-documents-found = Es wurden keine Dokumente gefunden, die den Kriterien entsprechen.
info-out-of-scope = Diese Anfrage liegt außerhalb des Bereichs des Baustellenüberwachungssystems.

# ------------------------------------------------------------
# AgentError messages
# ------------------------------------------------------------
error-cancelled = Die Anfrage wurde abgebrochen.

error-missing-object-id =
    Es wurde keine Objekt-ID übergeben.
    Bitte wählen Sie ein Objekt aus und versuchen Sie es erneut.

error-invalid-uuid =
    Der Wert „{ $raw }" ist keine gültige ID.
    Bitte versuchen Sie es erneut oder wenden Sie sich an den Support.

error-object-not-found =
    Das Objekt „{ $id }" wurde nicht gefunden.
    Bitte prüfen Sie die Objekt-ID und versuchen Sie es erneut.

error-no-documents-found =
    Es wurden keine Dokumente gefunden, die Ihren Kriterien entsprechen.
    Versuchen Sie einen anderen Zeitraum oder ein anderes Objekt.

error-llm-json-parse =
    Das KI-Modell hat eine unerwartete Antwort zurückgegeben, die nicht verarbeitet werden konnte.
    Details: { $detail }

error-template-render =
    Die Vorlage „{ $template }" konnte nicht dargestellt werden.
    Bitte wenden Sie sich an den Support, wenn dieses Problem weiterhin auftritt.

error-localization-key-missing =
    Fehlender Übersetzungsschlüssel: { $key }.
    Bitte melden Sie dies dem Support.

error-date-parse =
    Das Datum „{ $raw }" konnte nicht verarbeitet werden.
    Bitte verwenden Sie das Format TT.MM.JJJJ HH:MM:SS.

error-storage =
    Ein Speicherfehler ist aufgetreten: { $detail }.
    Bitte versuchen Sie es später erneut.

error-insufficient-descriptions =
    Für einen Vergleich werden mindestens 2 Berichtsbeschreibungen benötigt, aber nur { $found } waren verfügbar.

error-internal =
    Ein interner Fehler ist aufgetreten: { $detail }
    Bitte versuchen Sie es erneut oder wenden Sie sich an den Support.

# Context request — object selection
context-request-select-object = Bitte wählen Sie ein Objekt aus dem Baum aus, um fortzufahren.
context-request-select-object-hint = Öffnen Sie den Objektbaum und tippen Sie auf das gewünschte Element.

# Context request — current report selection
context-request-select-report = Bitte wählen Sie einen Bericht aus, den Sie beschreiben möchten.
context-request-select-report-hint = Wählen Sie einen Bericht aus der Liste aus, um die Beschreibung anzuzeigen.

# Context request — previous report selection
context-request-select-previous-report = Bitte wählen Sie zwei Berichte zum Vergleichen aus.
context-request-select-previous-report-hint = Wählen Sie einen älteren und einen neueren Bericht aus der Liste.

# Context request — second report missing
context-request-select-second-report = Für den Vergleich wird ein zweiter Bericht benötigt.
context-request-select-second-report-hint = Bitte wählen Sie einen anderen Bericht aus, um ihn mit dem aktuellen zu vergleichen.
