import { useState } from "react";
import type { Page } from "../types";

type NavigationState = {
  page: Page;
  direction: "forward" | "backward";
  animated: boolean;
};

const pageOrder: Page[] = ["library", "packs", "settings", "activity"];

export function usePageNavigation(initialPage: Page = "library") {
  const [navigation, setNavigation] = useState<NavigationState>({
    page: initialPage,
    direction: "forward",
    animated: false,
  });

  function setPage(page: Page) {
    setNavigation((current) => {
      if (current.page === page) return current;

      return {
        page,
        direction:
          pageOrder.indexOf(page) > pageOrder.indexOf(current.page)
            ? "forward"
            : "backward",
        animated: true,
      };
    });
  }

  return {
    page: navigation.page,
    direction: navigation.direction,
    animated: navigation.animated,
    setPage,
  };
}
