package example.web;

import example.models.Event;
import example.models.Note;
import example.models.Repo;

public class Family {
    public long find(Repo repo) {
        return repo.find(42L);
    }

    public Object made() {
        return Repo.of(1);
    }

    public String named(Repo repo) {
        return repo.name();
    }

    public int sized(Repo repo) {
        return repo.size();
    }

    public int coded(Event event) {
        return event.code();
    }

    public Note noted() {
        return new Note();
    }
}
